//! The reference for the walk of the sun's ray through the voxels that the shader makes (`docs/specs/contact-trace-spec.md`): which cells it enters, in what order, and
//! whether one of them blocks the sun.
//!
//! The shader works in `f32` and this in `f64`, so they agree wherever the ray does not pass within a rounding error of an edge or a corner of a cell; the tests of the shader
//! keep their probes off those, and the ties that are exact (a ray along a lattice plane, a diagonal through a corner) are settled the same way in both: x before y before z.

use voxel_space::{FaceDirection, VoxelCoordinate};

/// What the walk finds in a voxel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunCell {
    /// Nothing in it blocks the sun.
    Clear,
    /// All of it does.
    Blocks,
    /// Some of it does: the filled eighths of the voxel, one word for each layer of z, and bit `y * 8 + x` of it is whether the sub-cell `(x, y, z)` is filled (the layout of
    /// `block_models::Shape::cell_key`).
    Partial([u64; 8]),
}

/// Whether the sun's ray from a place on a face is blocked within `cells` cells, by voxels that block the sun whole or not at all.
///
/// `front` is the cell in front of the face (the one the ray starts in, which is never tested: it is where the place is, and what is in it is not between the place and the
/// sun), `fraction` is where the place is in that cell, from 0 to 1 along each axis (on the face, so one of them is 0 or 1), and `toward_sun` is the direction to the sun, which
/// need not be a unit vector. The walk enters the next cell along the ray `cells` times, and the place is in shadow if `blocks` says one of them blocks the sun. Where `blocks`
/// does not know (`None`: a brick that is not there) the walk ends and the sun is not blocked: the shadow map speaks for what is beyond.
pub fn sun_blocked(
    front: VoxelCoordinate,
    fraction: [f64; 3],
    toward_sun: [f64; 3],
    cells: u32,
    mut blocks: impl FnMut(VoxelCoordinate) -> Option<bool>,
) -> bool {
    walk(front, fraction, [0; 3], false, toward_sun, cells, |at| {
        blocks(at).map(|blocks| {
            if blocks {
                SunCell::Blocks
            } else {
                SunCell::Clear
            }
        })
    })
}

/// As [`sun_blocked`], for voxels that may block the sun in part: a slab, a stair, a post. `face` is the way the face the place is on looks. The place need not be on the
/// edge of its voxel: the top of a slab is in the middle of one, and `front` is then the slab's own voxel. The walk starts in the eighth of the voxel that is a sixteenth of a
/// voxel out of the face and, if the voxel blocks in part, goes on through the eighths of it that the ray crosses before it leaves (the first, where it starts, is not tested:
/// as the start cell is not); in each voxel it enters that blocks in part it goes through the eighths from the one the ray enters, which is tested. The voxels the budget
/// `cells` counts are those it enters, as in [`sun_blocked`].
pub fn sun_blocked_by_shapes(
    front: VoxelCoordinate,
    fraction: [f64; 3],
    face: FaceDirection,
    toward_sun: [f64; 3],
    cells: u32,
    blocks: impl FnMut(VoxelCoordinate) -> Option<SunCell>,
) -> bool {
    let mut start = [0usize; 3];
    for axis in 0..3 {
        let mut at = fraction[axis] * 8.0;
        if axis == face.axis() {
            // (A sixteenth of a voxel out of the face is half an eighth.)
            at += if face.is_positive() { 0.5 } else { -0.5 };
        }
        start[axis] = (at.floor() as i64).clamp(0, 7) as usize;
    }
    walk(front, fraction, start, false, toward_sun, cells, blocks)
}

/// As [`sun_blocked_by_shapes`], for a ray from a point in the lattice that is on no face of it: the point of a face of another grid, in this one's axes and a sixteenth of a
/// voxel out of that face. Nothing is left out: the voxel the point is in and the eighth of it are tested, since the lattice's own voxels are nowhere near the face the place is
/// on, and a place inside a voxel of this grid is in shade. The walk then enters `cells` voxels, as the others do. A voxel that is not known (`None`) is vacuum, and the walk goes on
/// through it: a grid has bricks only where it has something, and a ray from beside it crosses the bricks it has not to come to the ones it has.
pub fn sun_blocked_from_point(
    point: [f64; 3],
    toward_sun: [f64; 3],
    cells: u32,
    blocks: impl FnMut(VoxelCoordinate) -> Option<SunCell>,
) -> bool {
    let floor = point.map(f64::floor);
    let front = VoxelCoordinate::new(floor[0] as i128, floor[1] as i128, floor[2] as i128);
    let fraction: [f64; 3] = std::array::from_fn(|axis| point[axis] - floor[axis]);
    let start = fraction.map(|part| ((part * 8.0).floor() as i64).clamp(0, 7) as usize);
    walk(front, fraction, start, true, toward_sun, cells, blocks)
}

/// Whether the ray from `position` (in voxels from the lowest corner of a voxel), which starts in its eighth `sub`, meets a filled eighth of `mask` before it leaves the voxel.
/// The eighth it starts in is tested only if `test_first`. On a tie between boundaries the lowest axis goes first, as in the walk of voxels.
fn blocked_within_voxel(
    mask: &[u64; 8],
    position: [f64; 3],
    toward_sun: [f64; 3],
    mut sub: [usize; 3],
    test_first: bool,
) -> bool {
    let filled = |sub: [usize; 3]| mask[sub[2]] >> (sub[1] * 8 + sub[0]) & 1 == 1;
    if test_first && filled(sub) {
        return true;
    }
    let mut step = [0i64; 3];
    let mut next_boundary = [f64::INFINITY; 3];
    let mut boundary_spacing = [f64::INFINITY; 3];
    for axis in 0..3 {
        let direction = toward_sun[axis];
        if direction > 0.0 {
            step[axis] = 1;
            next_boundary[axis] = ((sub[axis] + 1) as f64 / 8.0 - position[axis]) / direction;
            boundary_spacing[axis] = 1.0 / (8.0 * direction);
        } else if direction < 0.0 {
            step[axis] = -1;
            next_boundary[axis] = (position[axis] - sub[axis] as f64 / 8.0) / -direction;
            boundary_spacing[axis] = 1.0 / (8.0 * -direction);
        }
    }
    loop {
        let axis = if next_boundary[0] <= next_boundary[1] && next_boundary[0] <= next_boundary[2] {
            0
        } else if next_boundary[1] <= next_boundary[2] {
            1
        } else {
            2
        };
        if next_boundary[axis].is_infinite() {
            return false;
        }
        let moved = sub[axis] as i64 + step[axis];
        if !(0..8).contains(&moved) {
            return false;
        }
        sub[axis] = moved as usize;
        next_boundary[axis] += boundary_spacing[axis];
        if filled(sub) {
            return true;
        }
    }
}

fn walk(
    front: VoxelCoordinate,
    fraction: [f64; 3],
    start_sub: [usize; 3],
    foreign: bool,
    toward_sun: [f64; 3],
    cells: u32,
    mut blocks: impl FnMut(VoxelCoordinate) -> Option<SunCell>,
) -> bool {
    // The start voxel: a face's is never tested (a voxel the place is in is not between it and the sun) and, if it blocks in part, is walked from the eighth the place is in
    // without testing that one; a point's (`foreign`) is tested, whole or by the eighth the point is in.
    if foreign || cells > 0 {
        match blocks(front) {
            Some(SunCell::Blocks) if foreign => return true,
            Some(SunCell::Partial(mask))
                if blocked_within_voxel(&mask, fraction, toward_sun, start_sub, foreign) =>
            {
                return true;
            }
            _ => {}
        }
    }
    let mut cell = [front.x(), front.y(), front.z()];
    let mut step = [0i128; 3];
    let mut next_boundary = [f64::INFINITY; 3];
    let mut boundary_spacing = [f64::INFINITY; 3];
    for axis in 0..3 {
        let direction = toward_sun[axis];
        if direction > 0.0 {
            step[axis] = 1;
            next_boundary[axis] = (1.0 - fraction[axis]) / direction;
            boundary_spacing[axis] = 1.0 / direction;
        } else if direction < 0.0 {
            step[axis] = -1;
            next_boundary[axis] = fraction[axis] / -direction;
            boundary_spacing[axis] = 1.0 / -direction;
        }
    }
    for _ in 0..cells {
        // The nearest boundary; on a tie the lowest axis.
        let axis = if next_boundary[0] <= next_boundary[1] && next_boundary[0] <= next_boundary[2] {
            0
        } else if next_boundary[1] <= next_boundary[2] {
            1
        } else {
            2
        };
        if next_boundary[axis].is_infinite() {
            // No direction at all: the ray goes nowhere.
            return false;
        }
        // (Where along the ray the voxel is entered.)
        let entered_at = next_boundary[axis];
        cell[axis] += step[axis];
        next_boundary[axis] += boundary_spacing[axis];
        let Ok(entered) = VoxelCoordinate::try_new(cell[0], cell[1], cell[2]) else {
            return false;
        };
        match blocks(entered) {
            Some(SunCell::Blocks) => return true,
            Some(SunCell::Clear) => {}
            // (A voxel nothing is known of ends the walk of a face's own grid, whose bricks are all the bricks there are about it; for another's it is vacuum: its bricks are
            // only where it has them.)
            None if foreign => {}
            Some(SunCell::Partial(mask)) => {
                // The place of the ray at the boundary, in voxels from the entered voxel's corner; the axis it came across is exactly 0 or 1.
                let start = [front.x(), front.y(), front.z()];
                let mut position = [0.0; 3];
                let mut sub = [0usize; 3];
                for other in 0..3 {
                    let relative = (cell[other] - start[other]) as f64;
                    position[other] = if other == axis {
                        if step[axis] > 0 { 0.0 } else { 1.0 }
                    } else {
                        (fraction[other] + entered_at * toward_sun[other] - relative)
                            .clamp(0.0, 1.0)
                    };
                    sub[other] = if other == axis {
                        if step[axis] > 0 { 0 } else { 7 }
                    } else {
                        ((position[other] * 8.0).floor() as i64).clamp(0, 7) as usize
                    };
                }
                if blocked_within_voxel(&mask, position, toward_sun, sub, true) {
                    return true;
                }
            }
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use proptest::prelude::*;

    use super::*;

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    /// A world where exactly these cells block the sun, and every other cell is known.
    fn world_of(solids: &[[i128; 3]]) -> impl FnMut(VoxelCoordinate) -> Option<bool> + '_ {
        move |at| Some(solids.contains(&[at.x(), at.y(), at.z()]))
    }

    #[test]
    fn a_wall_beside_the_place_blocks_a_rising_ray_and_one_too_far_does_not() {
        // A floor point at the foot of a wall one cell to +x, the sun to +x and 45 degrees up: the ray rises as it crosses, and enters the wall's cell at once.
        let wall = [[1, 0, 0], [1, 1, 0]];
        assert!(sun_blocked(
            voxel(0, 0, 0),
            [0.5, 0.0, 0.5],
            [1.0, 1.0, 0.0],
            1,
            world_of(&wall)
        ));
        // Three cells off and 45 degrees up, the ray is over a wall of height 1 by then; a wall of height 3 still has it.
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5, 0.0, 0.5],
            [1.0, 1.0, 0.0],
            8,
            world_of(&[[3, 0, 0]])
        ));
        assert!(sun_blocked(
            voxel(0, 0, 0),
            [0.5, 0.0, 0.5],
            [1.0, 1.0, 0.0],
            8,
            world_of(&[[3, 2, 0]])
        ));
    }

    #[test]
    fn the_start_cell_is_never_tested_and_zero_cells_find_nothing() {
        let solid = [[0, 0, 0], [1, 0, 0]];
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [1.0, 0.0, 0.0],
            0,
            world_of(&solid)
        ));
        assert!(sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [1.0, 0.0, 0.0],
            1,
            world_of(&solid)
        ));
        assert!(!sun_blocked(
            voxel(1, 0, 0),
            [0.5; 3],
            [-1.0, 0.0, 0.0],
            0,
            world_of(&solid)
        ));
        // (Walking toward -x from cell 1 enters cell 0, which blocks.)
        assert!(sun_blocked(
            voxel(1, 0, 0),
            [0.5; 3],
            [-1.0, 0.0, 0.0],
            1,
            world_of(&solid)
        ));
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [0.0, 0.0, 0.0],
            9,
            world_of(&solid)
        ));
    }

    #[test]
    fn a_ray_along_a_lattice_plane_stays_in_its_row_and_does_not_step_sideways() {
        // The sun straight along +x: only the cells of the row x, 0, 0 are entered. A wall beside the row is not in the way; one in it is.
        let beside = [[1, 1, 0], [1, -1, 0], [1, 0, 1], [1, 0, -1], [2, 0, 5]];
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [1.0, 0.0, 0.0],
            5,
            world_of(&beside)
        ));
        assert!(sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [1.0, 0.0, 0.0],
            5,
            world_of(&[[4, 0, 0]])
        ));
        // Straight up, as at noon: the same, with two components exactly zero.
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [0.0, 1.0, 0.0],
            5,
            world_of(&[[1, 1, 0], [0, 1, 1], [-1, 1, 0]])
        ));
        assert!(sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [0.0, 1.0, 0.0],
            5,
            world_of(&[[0, 3, 0]])
        ));
        // And one the other way: toward -z.
        assert!(sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [0.0, 0.0, -1.0],
            5,
            world_of(&[[0, 0, -4]])
        ));
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [0.0, 0.0, -1.0],
            5,
            world_of(&[[0, 0, 1], [0, 1, -1]])
        ));
    }

    #[test]
    fn a_diagonal_through_a_corner_goes_x_then_y_then_z() {
        // From the lowest corner of a cell straight along the diagonal, the three boundaries are met together; the walk takes x, then y, then z, so the cells it enters are
        // (1,0,0), (1,1,0) and (1,1,1): never (0,1,0), (0,0,1), (0,1,1) or (1,0,1), though the true ray touches them at a point.
        let order = |solids: &[[i128; 3]], cells| {
            sun_blocked(
                voxel(0, 0, 0),
                [0.0; 3],
                [1.0, 1.0, 1.0],
                cells,
                world_of(solids),
            )
        };
        assert!(order(&[[1, 0, 0]], 1));
        assert!(!order(&[[0, 1, 0], [0, 0, 1], [0, 1, 1], [1, 0, 1]], 9));
        assert!(!order(&[[1, 1, 0]], 1));
        assert!(order(&[[1, 1, 0]], 2));
        assert!(!order(&[[1, 1, 1]], 2));
        assert!(order(&[[1, 1, 1]], 3));
    }

    #[test]
    fn a_cell_that_is_not_known_ends_the_walk_and_leaves_the_sun_unblocked() {
        let known_to_one_cell = |at: VoxelCoordinate| (at.x() <= 1).then_some(at.x() == 3);
        assert!(!sun_blocked(
            voxel(0, 0, 0),
            [0.5; 3],
            [1.0, 0.0, 0.0],
            9,
            known_to_one_cell
        ));
    }

    #[test]
    fn it_works_far_from_the_origin_and_across_negative_coordinates() {
        let far = 1_i128 << 100;
        let wall = [[far + 2, -far, 7]];
        assert!(sun_blocked(
            voxel(far, -far, 7),
            [0.5; 3],
            [1.0, 0.0, 0.0],
            3,
            world_of(&wall)
        ));
        assert!(sun_blocked(
            voxel(-far, 0, 0),
            [0.5; 3],
            [-1.0, 0.0, 0.0],
            3,
            world_of(&[[-far - 3, 0, 0]])
        ));
    }

    /// The oracle: every cell of the box the ray crosses, by the slab method for each cell alone (no stepping), in the order the ray enters them. The cells that the walk of
    /// `cells` steps enters are the first `cells` after the start cell.
    fn cells_entered_by_slabs(
        start: [i128; 3],
        fraction: [f64; 3],
        direction: [f64; 3],
        reach: i128,
    ) -> Vec<([i128; 3], f64)> {
        let mut entered = Vec::new();
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                for dz in -reach..=reach {
                    let offset = [dx, dy, dz];
                    // The ray from the place, as a line in units of cells from the start cell's lowest corner.
                    let mut near = f64::NEG_INFINITY;
                    let mut far = f64::INFINITY;
                    let mut inside = true;
                    for axis in 0..3 {
                        let low = offset[axis] as f64;
                        let origin = fraction[axis];
                        if direction[axis] == 0.0 {
                            if !(low < origin && origin < low + 1.0) {
                                inside = false;
                            }
                            continue;
                        }
                        let a = (low - origin) / direction[axis];
                        let b = (low + 1.0 - origin) / direction[axis];
                        near = near.max(a.min(b));
                        far = far.min(a.max(b));
                    }
                    if inside && near < far && far > 0.0 && offset != [0, 0, 0] {
                        entered.push(([start[0] + dx, start[1] + dy, start[2] + dz], near));
                    }
                }
            }
        }
        entered.sort_by(|a, b| a.1.total_cmp(&b.1));
        entered
    }

    proptest! {
        /// For random solids in a small box, a random place in a random cell and a random direction (off the exact ties), the walk says blocked exactly when one of the first
        /// `cells` cells the ray enters, found for every cell alone by the slab method, is solid.
        #[test]
        fn the_walk_blocks_exactly_when_one_of_the_first_cells_the_ray_enters_is_solid(
            fraction in proptest::array::uniform3(0.001f64..0.999),
            direction in proptest::array::uniform3(-1.0f64..1.0),
            cells in 1u32..9,
            solids in proptest::collection::hash_set(proptest::array::uniform3(-9i128..10), 0..120),
        ) {
            prop_assume!(direction.iter().any(|component| component.abs() > 1e-3));
            let start = [0i128; 3];
            let by_walk = sun_blocked(
                voxel(0, 0, 0),
                fraction,
                direction,
                cells,
                |at| Some(solids.contains(&[at.x(), at.y(), at.z()])),
            );
            let entered = cells_entered_by_slabs(start, fraction, direction, 9);
            let by_slabs = entered
                .iter()
                .take(cells as usize)
                .any(|(cell, _)| solids.contains(cell));
            prop_assert_eq!(by_walk, by_slabs);
        }

        /// The same with rays along an axis or in a plane of the lattice (a component exactly zero), the case the ties are about: the cells are those of the row or plane.
        #[test]
        fn the_walk_agrees_with_the_slab_method_for_rays_with_a_component_of_exactly_zero(
            fraction in proptest::array::uniform3(0.001f64..0.999),
            direction in proptest::array::uniform3(-1.0f64..1.0),
            zeroed in proptest::collection::vec(proptest::bool::ANY, 3),
            cells in 1u32..9,
            solids in proptest::collection::hash_set(proptest::array::uniform3(-9i128..10), 0..160),
        ) {
            let mut direction = direction;
            for axis in 0..3 {
                if zeroed[axis] {
                    direction[axis] = 0.0;
                }
            }
            prop_assume!(direction.iter().any(|component| component.abs() > 1e-3));
            let by_walk = sun_blocked(
                voxel(0, 0, 0),
                fraction,
                direction,
                cells,
                |at| Some(solids.contains(&[at.x(), at.y(), at.z()])),
            );
            let entered = cells_entered_by_slabs([0; 3], fraction, direction, 9);
            let by_slabs = entered
                .iter()
                .take(cells as usize)
                .any(|(cell, _)| solids.contains(cell));
            prop_assert_eq!(by_walk, by_slabs);
        }
    }

    /// The eighths of the lower half of a voxel, of the upper half, and of a stair (the lower half and the upper half of the part from x = 4 on): the layout of
    /// `Shape::cell_key`, one word for each layer of z, bit `y * 8 + x`.
    const BOTTOM_SLAB: [u64; 8] = [0x0000_0000_FFFF_FFFF; 8];
    const TOP_SLAB: [u64; 8] = [0xFFFF_FFFF_0000_0000; 8];
    const STAIR_UP_TO_PLUS_X: [u64; 8] = [0xF0F0_F0F0_FFFF_FFFF; 8];

    fn shaped_world(
        cells: &[([i128; 3], SunCell)],
    ) -> impl FnMut(VoxelCoordinate) -> Option<SunCell> + '_ {
        move |at| {
            Some(
                cells
                    .iter()
                    .find(|(cell, _)| *cell == [at.x(), at.y(), at.z()])
                    .map_or(SunCell::Clear, |(_, found)| *found),
            )
        }
    }

    #[test]
    fn a_ray_across_a_bottom_slab_is_blocked_when_it_goes_through_the_lower_half_and_not_when_it_passes_over()
     {
        // A floor place at the foot of a slab one voxel to +x (the slab fills the lower half of its voxel). The ray enters the slab's voxel at x = 1, at the height it has risen by
        // then: under a half it is in the slab; over a half it passes above it, and the voxel beyond is clear.
        let world = [([1, 0, 0], SunCell::Partial(BOTTOM_SLAB))];
        let blocked = |rise: f64| {
            sun_blocked_by_shapes(
                voxel(0, 0, 0),
                [0.5, 0.0, 0.5],
                FaceDirection::PositiveY,
                [1.0, rise, 0.0],
                4,
                shaped_world(&world),
            )
        };
        assert!(blocked(0.9));
        assert!(!blocked(1.2));
        // A whole cube in the same place blocks both: the control that it is the shape that lets the second pass.
        let cube = [([1, 0, 0], SunCell::Blocks)];
        assert!(sun_blocked_by_shapes(
            voxel(0, 0, 0),
            [0.5, 0.0, 0.5],
            FaceDirection::PositiveY,
            [1.0, 1.2, 0.0],
            4,
            shaped_world(&cube),
        ));
    }

    #[test]
    fn a_ray_over_a_top_slab_that_never_reaches_the_upper_half_is_not_blocked_and_one_that_does_is()
    {
        let world = [([1, 0, 0], SunCell::Partial(TOP_SLAB))];
        let blocked = |rise: f64| {
            sun_blocked_by_shapes(
                voxel(0, 0, 0),
                [0.5, 0.0, 0.5],
                FaceDirection::PositiveY,
                [1.0, rise, 0.0],
                4,
                shaped_world(&world),
            )
        };
        // Entering at x = 1 after rising 0.5 * rise (from the middle of the voxel), and leaving at x = 2 after rising 1.5 * rise: it is over a half (y = 0.5) there when the
        // rise is more than 1/3.
        assert!(!blocked(0.3));
        assert!(blocked(0.6));
    }

    #[test]
    fn a_place_on_the_lower_step_of_a_stair_is_shadowed_by_the_upper_step_in_the_same_voxel_and_not_by_the_air_above_the_lower_one()
     {
        let world = [([0, 0, 0], SunCell::Partial(STAIR_UP_TO_PLUS_X))];
        let from_the_tread = |sun: [f64; 3]| {
            sun_blocked_by_shapes(
                voxel(0, 0, 0),
                [0.25, 0.5, 0.5],
                FaceDirection::PositiveY,
                sun,
                4,
                shaped_world(&world),
            )
        };
        // The sun over the high step: the ray meets its riser a quarter of a voxel along.
        assert!(from_the_tread([1.0, 0.3, 0.0]));
        // The sun the other way, over the open side: nothing is in the way.
        assert!(!from_the_tread([-1.0, 0.3, 0.0]));
        // Straight up from the tread: nothing over it.
        assert!(!from_the_tread([0.0, 1.0, 0.0]));
    }

    #[test]
    fn the_start_voxel_is_not_walked_with_no_cells_and_a_partial_voxel_is_not_tested_in_the_eighth_it_starts_in()
     {
        let world = [([0, 0, 0], SunCell::Partial(BOTTOM_SLAB))];
        // No cells: nothing is found, though the ray would meet the riser of a stair.
        let stairs = [([0, 0, 0], SunCell::Partial(STAIR_UP_TO_PLUS_X))];
        assert!(!sun_blocked_by_shapes(
            voxel(0, 0, 0),
            [0.25, 0.5, 0.5],
            FaceDirection::PositiveY,
            [1.0, 0.3, 0.0],
            0,
            shaped_world(&stairs),
        ));
        // On the top of a slab, the eighth the place starts in (just over the top) is clear, and so is everything the rising ray crosses after it.
        assert!(!sun_blocked_by_shapes(
            voxel(0, 0, 0),
            [0.5, 0.5, 0.5],
            FaceDirection::PositiveY,
            [1.0, 0.1, 0.3],
            4,
            shaped_world(&world),
        ));
    }

    /// Whether the ray from `fraction` along `direction` goes through the inside of the box from `low` to `high` (in voxels from the start voxel's lowest corner), at some
    /// distance ahead of the place: the slab method for the one box.
    fn ray_goes_through_box(
        fraction: [f64; 3],
        direction: [f64; 3],
        low: [f64; 3],
        high: [f64; 3],
    ) -> bool {
        let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
        for axis in 0..3 {
            if direction[axis] == 0.0 {
                if !(low[axis] < fraction[axis] && fraction[axis] < high[axis]) {
                    return false;
                }
                continue;
            }
            let a = (low[axis] - fraction[axis]) / direction[axis];
            let b = (high[axis] - fraction[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
        near < far && far > 0.0
    }

    /// The oracle for shapes: the voxels the walk enters, found for every cell alone (`cells_entered_by_slabs`), and the start voxel; in each, the ray against the box of every
    /// filled eighth (and against the whole voxel, if it blocks whole), each by the slab method alone. The eighth the walk starts in is left out, as it is.
    fn blocked_by_boxes(
        world: &std::collections::HashMap<[i128; 3], SunCell>,
        fraction: [f64; 3],
        face: FaceDirection,
        direction: [f64; 3],
        cells: u32,
    ) -> bool {
        let mut start_sub = [0usize; 3];
        for axis in 0..3 {
            let mut at = fraction[axis];
            if axis == face.axis() {
                at += if face.is_positive() {
                    1.0 / 16.0
                } else {
                    -1.0 / 16.0
                };
            }
            start_sub[axis] = ((at * 8.0).floor() as i64).clamp(0, 7) as usize;
        }
        blocked_by_boxes_from(world, fraction, start_sub, false, direction, cells)
    }

    /// The same for a ray that starts in the eighth `start_sub` of the voxel 0, 0, 0 and, if `start_tested`, finds what is in that voxel and that eighth too (a ray from a
    /// point in free space, and not from a face).
    fn blocked_by_boxes_from(
        world: &std::collections::HashMap<[i128; 3], SunCell>,
        fraction: [f64; 3],
        start_sub: [usize; 3],
        start_tested: bool,
        direction: [f64; 3],
        cells: u32,
    ) -> bool {
        let cell_at = |cell: [i128; 3]| world.get(&cell).copied().unwrap_or(SunCell::Clear);
        let mut voxels: Vec<([i128; 3], bool)> = vec![([0; 3], true)];
        for (cell, _) in cells_entered_by_slabs([0; 3], fraction, direction, 9)
            .into_iter()
            .take(cells as usize)
        {
            voxels.push((cell, false));
        }
        if cells == 0 && !start_tested {
            voxels.clear();
        }
        for (cell, is_start) in voxels {
            let corner = cell.map(|component| component as f64);
            match cell_at(cell) {
                SunCell::Clear => {}
                SunCell::Blocks => {
                    if (start_tested || !is_start)
                        && ray_goes_through_box(
                            fraction,
                            direction,
                            corner,
                            corner.map(|component| component + 1.0),
                        )
                    {
                        return true;
                    }
                }
                SunCell::Partial(mask) => {
                    for (z, layer) in mask.iter().enumerate() {
                        for y in 0..8 {
                            for x in 0..8 {
                                if layer >> (y * 8 + x) & 1 == 0
                                    || (is_start && !start_tested && [x, y, z] == start_sub)
                                {
                                    continue;
                                }
                                let low = [
                                    corner[0] + x as f64 / 8.0,
                                    corner[1] + y as f64 / 8.0,
                                    corner[2] + z as f64 / 8.0,
                                ];
                                if ray_goes_through_box(
                                    fraction,
                                    direction,
                                    low,
                                    low.map(|component| component + 0.125),
                                ) {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn sparse_mask(a: [u64; 8], b: [u64; 8], c: [u64; 8]) -> [u64; 8] {
        std::array::from_fn(|layer| a[layer] & b[layer] & c[layer])
    }

    proptest! {
        /// For random voxels (clear, whole, or with random eighths filled) in a small box, a random place in the start voxel on a random face (its axis ahead of the sun, as the
        /// shader asks), and a random direction, the walk blocks exactly when the ray goes through a filled eighth of the voxels it enters or of the start voxel, found for every
        /// eighth alone by the slab method.
        #[test]
        fn the_walk_through_shapes_blocks_exactly_when_the_ray_goes_through_a_filled_eighth_of_the_voxels_it_enters(
            fraction in proptest::array::uniform3(0.001f64..0.999),
            direction in proptest::array::uniform3(-1.0f64..1.0),
            zeroed in proptest::collection::vec(proptest::bool::ANY, 3),
            face_index in 0usize..6,
            plane in 0u8..8,
            cells in 0u32..9,
            kinds in proptest::collection::hash_map(
                proptest::array::uniform3(-5i128..6),
                (0u8..10, any::<[u64; 8]>(), any::<[u64; 8]>(), any::<[u64; 8]>()),
                0..400,
            ),
            start_mask in (any::<[u64; 8]>(), any::<[u64; 8]>()),
        ) {
            let face = FaceDirection::ALL[face_index];
            // The face is in a plane that is a multiple of an eighth: from the voxel's lower side up to the seventh for a face that looks up the axis (the lowest is the
            // voxel's own boundary, with the voxel in front of it), and from the first to the voxel's upper side for one that looks down it.
            let mut fraction = fraction;
            fraction[face.axis()] = f64::from(plane + u8::from(!face.is_positive())) / 8.0;
            let mut direction = direction;
            for axis in 0..3 {
                // (A component of exactly zero, but not along the face's axis: a ray along the face looks at nothing.)
                if zeroed[axis] && axis != face.axis() {
                    direction[axis] = 0.0;
                }
            }
            // The sun is ahead of the face.
            let sign = if face.is_positive() { 1.0 } else { -1.0 };
            direction[face.axis()] = sign * (0.05 + direction[face.axis()].abs());
            let mut world: std::collections::HashMap<[i128; 3], SunCell> = kinds
                .into_iter()
                .map(|(cell, (kind, a, b, c))| {
                    let found = match kind {
                        0..=4 => SunCell::Clear,
                        5 => SunCell::Blocks,
                        _ => SunCell::Partial(sparse_mask(a, b, c)),
                    };
                    (cell, found)
                })
                .collect();
            world.insert([0; 3], SunCell::Partial(sparse_mask(start_mask.0, start_mask.1, [u64::MAX; 8])));
            let by_walk = sun_blocked_by_shapes(
                voxel(0, 0, 0),
                fraction,
                face,
                direction,
                cells,
                |at| Some(world.get(&[at.x(), at.y(), at.z()]).copied().unwrap_or(SunCell::Clear)),
            );
            let by_boxes = blocked_by_boxes(&world, fraction, face, direction, cells);
            prop_assert_eq!(by_walk, by_boxes);
        }
    }

    #[test]
    fn a_ray_from_a_point_in_free_space_finds_what_is_at_the_point_and_what_is_ahead() {
        let from = |point: [f64; 3], sun: [f64; 3], world: &[([i128; 3], SunCell)]| {
            sun_blocked_from_point(point, sun, 4, shaped_world(world))
        };
        let cube = [([0, 0, 0], SunCell::Blocks)];
        // A point inside a cube is blocked at once (a face's start cell is never tested; the start of a foreign lattice's walk is).
        assert!(from([0.5, 0.5, 0.5], [0.0, 1.0, 0.0], &cube));
        assert!(!from([1.5, 0.5, 0.5], [0.0, 1.0, 0.0], &cube));
        // Beside the cube, toward it: blocked; away from it: not.
        assert!(from([-0.5, 0.5, 0.5], [1.0, 0.0, 0.0], &cube));
        assert!(!from([-0.5, 0.5, 0.5], [-1.0, 0.0, 0.0], &cube));
        // Inside a bottom slab: in its lower half the point is inside it, in its upper half it is over it, and a ray that rises has left.
        let slab = [([0, 0, 0], SunCell::Partial(BOTTOM_SLAB))];
        assert!(from([0.5, 0.25, 0.5], [0.0, 1.0, 0.0], &slab));
        assert!(!from([0.5, 0.75, 0.5], [0.0, 1.0, 0.0], &slab));
        // Over the slab, a ray that goes down into it is blocked and one that goes along over it is not.
        assert!(from([0.5, 0.75, 0.5], [0.3, -1.0, 0.0], &slab));
        assert!(!from([0.5, 0.75, 0.5], [1.0, 0.0, 0.0], &slab));
        // A voxel that is not known is vacuum to it, at the start too, and the walk goes on through it: another grid's bricks are only where it has them, and a ray from beside
        // the grid has to cross the bricks it does not have to get to the ones it does.
        let known_from_the_third = |at: VoxelCoordinate| {
            (at.x() >= 3).then(|| {
                if at.x() == 3 {
                    SunCell::Blocks
                } else {
                    SunCell::Clear
                }
            })
        };
        assert!(sun_blocked_from_point(
            [0.5, 0.5, 0.5],
            [1.0, 0.0, 0.0],
            4,
            known_from_the_third
        ));
        // ... but only as far as the cells go: three cells reach x = 3 from x = 0, two do not.
        assert!(!sun_blocked_from_point(
            [0.5, 0.5, 0.5],
            [1.0, 0.0, 0.0],
            2,
            known_from_the_third
        ));
        assert!(!sun_blocked_from_point(
            [0.5, 0.5, 0.5],
            [1.0, 0.0, 0.0],
            4,
            |_| None
        ));
    }

    proptest! {
        /// For random voxels and a random point in the voxel 0, 0, 0 (in free space: no face, so nothing is left out), the walk blocks exactly when the ray goes through a filled
        /// eighth, or a whole voxel, of the start voxel or of the voxels it enters, found for every eighth alone.
        #[test]
        fn a_walk_from_a_point_blocks_exactly_when_the_ray_goes_through_something_filled(
            fraction in proptest::array::uniform3(0.001f64..0.999),
            direction in proptest::array::uniform3(-1.0f64..1.0),
            zeroed in proptest::collection::vec(proptest::bool::ANY, 3),
            cells in 0u32..9,
            kinds in proptest::collection::hash_map(
                proptest::array::uniform3(-5i128..6),
                (0u8..10, any::<[u64; 8]>(), any::<[u64; 8]>(), any::<[u64; 8]>()),
                0..400,
            ),
        ) {
            let mut direction = direction;
            for axis in 0..3 {
                if zeroed[axis] {
                    direction[axis] = 0.0;
                }
            }
            prop_assume!(direction.iter().any(|component| component.abs() > 1e-3));
            let world: std::collections::HashMap<[i128; 3], SunCell> = kinds
                .into_iter()
                .map(|(cell, (kind, a, b, c))| {
                    let found = match kind {
                        0..=4 => SunCell::Clear,
                        5 => SunCell::Blocks,
                        _ => SunCell::Partial(sparse_mask(a, b, c)),
                    };
                    (cell, found)
                })
                .collect();
            let by_walk = sun_blocked_from_point(fraction, direction, cells, |at| {
                Some(world.get(&[at.x(), at.y(), at.z()]).copied().unwrap_or(SunCell::Clear))
            });
            let start_sub = fraction.map(|part| ((part * 8.0).floor() as i64).clamp(0, 7) as usize);
            let by_boxes = blocked_by_boxes_from(&world, fraction, start_sub, true, direction, cells);
            prop_assert_eq!(by_walk, by_boxes);
        }
    }

    #[test]
    fn the_oracle_itself_finds_the_cells_of_a_known_ray_in_order() {
        // From the middle of cell 0 along +x: cells 1, 2, 3, ... in order, nothing else.
        let entered = cells_entered_by_slabs([0; 3], [0.5; 3], [1.0, 0.0, 0.0], 4);
        let cells: Vec<[i128; 3]> = entered.iter().map(|(cell, _)| *cell).collect();
        assert_eq!(cells, [[1, 0, 0], [2, 0, 0], [3, 0, 0], [4, 0, 0]]);
        // A diagonal ray from the middle of a cell enters 3 cells for every cell of its length along each axis (a staircase), never the same cell twice.
        let entered = cells_entered_by_slabs([0; 3], [0.25, 0.5, 0.75], [1.0, 0.6, 0.3], 6);
        let cells: HashSet<[i128; 3]> = entered.iter().map(|(cell, _)| *cell).collect();
        assert_eq!(cells.len(), entered.len());
    }
}
