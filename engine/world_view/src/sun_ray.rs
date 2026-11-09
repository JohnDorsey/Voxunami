//! The reference for the walk of the sun's ray through the voxels that the shader makes (`docs/specs/contact-trace-spec.md`): which cells it enters, in what order, and
//! whether one of them blocks the sun.
//!
//! The shader works in `f32` and this in `f64`, so they agree wherever the ray does not pass within a rounding error of an edge or a corner of a cell; the tests of the shader
//! keep their probes off those, and the ties that are exact (a ray along a lattice plane, a diagonal through a corner) are settled the same way in both: x before y before z.

use voxel_space::VoxelCoordinate;

/// Whether the sun's ray from a place on a face is blocked within `cells` cells.
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
        cell[axis] += step[axis];
        next_boundary[axis] += boundary_spacing[axis];
        let Ok(entered) = VoxelCoordinate::try_new(cell[0], cell[1], cell[2]) else {
            return false;
        };
        match blocks(entered) {
            Some(true) => return true,
            Some(false) => {}
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
