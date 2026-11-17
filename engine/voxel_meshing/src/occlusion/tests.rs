use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use proptest::prelude::*;
use voxel_space::FaceDirection;

use super::*;

/// A world of cubes that occlude, given by the set of cells that do.
#[derive(Debug)]
struct Cells(HashSet<[i64; 3]>);

impl Cells {
    fn occludes(&self, cell: [i64; 3]) -> bool {
        self.0.contains(&cell)
    }

    fn levels(&self, voxel: [i64; 3], face: FaceDirection) -> [u8; 4] {
        corner_occlusion(|cell| self.occludes(cell), voxel, face).levels()
    }

    /// Every face that is drawn: a cube that occludes with air in front of the face.
    fn drawn_faces(&self) -> Vec<([i64; 3], FaceDirection)> {
        let mut faces = Vec::new();
        for &voxel in &self.0 {
            for face in FaceDirection::ALL {
                if !self.occludes(step(voxel, face)) {
                    faces.push((voxel, face));
                }
            }
        }
        faces.sort_by_key(|&(voxel, face)| (voxel, face.index()));
        faces
    }
}

fn step(cell: [i64; 3], face: FaceDirection) -> [i64; 3] {
    let mut next = cell;
    next[face.axis()] += if face.is_positive() { 1 } else { -1 };
    next
}

/// Where corner `corner` (0 to 3, `u` fastest) of the face of `voxel` is in the world, in whole voxels from the origin.
fn vertex_position(voxel: [i64; 3], face: FaceDirection, corner: usize) -> [i64; 3] {
    let (u_axis, v_axis) = quad_axes(face);
    let mut position = voxel;
    position[face.axis()] += i64::from(face.is_positive());
    position[u_axis] += (corner & 1) as i64;
    position[v_axis] += (corner >> 1) as i64;
    position
}

/// The same levels worked out another way, from the vertex's place in the world and not from the face's own axes: the four cells of the layer in front that meet at the vertex,
/// told apart by how they sit against the cell the face looks at.
fn reference_levels(world: &Cells, voxel: [i64; 3], face: FaceDirection) -> [u8; 4] {
    let (u_axis, v_axis) = quad_axes(face);
    let front = step(voxel, face);
    let mut levels = [0; 4];
    for (corner, level) in levels.iter_mut().enumerate() {
        let vertex = vertex_position(voxel, face, corner);
        let (mut beside_in_u, mut beside_in_v, mut diagonal) = (false, false, false);
        for u in [vertex[u_axis] - 1, vertex[u_axis]] {
            for v in [vertex[v_axis] - 1, vertex[v_axis]] {
                let mut cell = front;
                cell[u_axis] = u;
                cell[v_axis] = v;
                match (u == front[u_axis], v == front[v_axis]) {
                    (true, true) => {}
                    (false, true) => beside_in_u = world.occludes(cell),
                    (true, false) => beside_in_v = world.occludes(cell),
                    (false, false) => diagonal = world.occludes(cell),
                }
            }
        }
        *level = if beside_in_u && beside_in_v {
            3
        } else {
            u8::from(beside_in_u) + u8::from(beside_in_v) + u8::from(diagonal)
        };
    }
    levels
}

/// A floor of stone up to and including y = 0 (as far as the tests look), and the given cells above it.
fn floor_with(above: &[[i64; 3]]) -> Cells {
    let mut cells = HashSet::new();
    for x in -6..=6 {
        for z in -6..=6 {
            for y in -2..=0 {
                cells.insert([x, y, z]);
            }
        }
    }
    cells.extend(above.iter().copied());
    Cells(cells)
}

/// The positions of the corners of a face that have this level, lowest corner index first.
fn corners_at_level(
    world: &Cells,
    voxel: [i64; 3],
    face: FaceDirection,
    level: u8,
) -> Vec<[i64; 3]> {
    let levels = world.levels(voxel, face);
    (0..4)
        .filter(|&corner| levels[corner] == level)
        .map(|corner| vertex_position(voxel, face, corner))
        .collect()
}

#[test]
fn open_ground_has_no_occlusion_on_any_face() {
    let ground = floor_with(&[]);
    for x in -3..=3 {
        for z in -3..=3 {
            assert_eq!(ground.levels([x, 0, z], FaceDirection::PositiveY), [0; 4]);
        }
    }
    let lone = Cells(HashSet::from([[0, 0, 0]]));
    for face in FaceDirection::ALL {
        assert_eq!(lone.levels([0, 0, 0], face), [0; 4]);
    }
}

#[test]
fn a_block_on_a_flat_floor_darkens_only_the_corner_that_touches_it_of_each_diagonal_neighbour() {
    // The block is the cube from (0, 1, 0) to (1, 2, 1). The floor's top faces are at y = 1.
    let world = floor_with(&[[0, 1, 0]]);
    for (side_x, side_z) in [(1, 1), (-1, -1), (1, -1), (-1, 1)] {
        let voxel = [side_x, 0, side_z];
        // The corner of the block that the neighbour's face touches: the neighbour's edge that faces the block.
        let touching = [i64::from(side_x > 0), 1, i64::from(side_z > 0)];
        assert_eq!(
            corners_at_level(&world, voxel, FaceDirection::PositiveY, 1),
            vec![touching],
            "level 1 corners of the floor face at {voxel:?}"
        );
        assert_eq!(
            world
                .levels(voxel, FaceDirection::PositiveY)
                .iter()
                .sum::<u8>(),
            1,
            "the other three corners are not occluded at all"
        );
    }
}

#[test]
fn a_block_on_a_flat_floor_darkens_the_two_corners_on_the_edge_it_touches_of_each_edge_neighbour() {
    let world = floor_with(&[[0, 1, 0]]);
    let cases = [
        ([1, 0, 0], [[1, 1, 0], [1, 1, 1]]),
        ([-1, 0, 0], [[0, 1, 0], [0, 1, 1]]),
        ([0, 0, 1], [[0, 1, 1], [1, 1, 1]]),
        ([0, 0, -1], [[0, 1, 0], [1, 1, 0]]),
    ];
    for (voxel, expected) in cases {
        let mut found = corners_at_level(&world, voxel, FaceDirection::PositiveY, 1);
        found.sort();
        assert_eq!(found, expected.to_vec(), "face at {voxel:?}");
        assert_eq!(
            world
                .levels(voxel, FaceDirection::PositiveY)
                .iter()
                .sum::<u8>(),
            2
        );
    }
}

#[test]
fn the_face_under_a_block_is_the_only_one_that_the_block_hides_and_the_rest_of_the_floor_is_untouched()
 {
    let world = floor_with(&[[0, 1, 0]]);
    assert!(
        !world
            .drawn_faces()
            .contains(&([0, 0, 0], FaceDirection::PositiveY))
    );
    for (x, z) in [(2, 0), (0, 2), (2, 2), (-2, 1), (3, 3)] {
        assert_eq!(world.levels([x, 0, z], FaceDirection::PositiveY), [0; 4]);
    }
}

#[test]
fn what_each_combination_of_side_and_corner_cells_gives_one_corner() {
    // The face of the stone at the origin that looks up; its corner 3 is at (1, 1, 1), with the cell it looks at at (0, 1, 0), its sides (0, 1, 1) and (1, 1, 0), and the
    // diagonal cell (1, 1, 1).
    let side_along_u = [0, 1, 1];
    let side_along_v = [1, 1, 0];
    let diagonal = [1, 1, 1];
    let cases: [(&[[i64; 3]], u8); 8] = [
        (&[], 0),
        (&[side_along_u], 1),
        (&[side_along_v], 1),
        (&[diagonal], 1),
        (&[side_along_u, diagonal], 2),
        (&[side_along_v, diagonal], 2),
        // Both sides closed: nothing gets round the corner, so it is as dark as it goes whether or not the diagonal is.
        (&[side_along_u, side_along_v], 3),
        (&[side_along_u, side_along_v, diagonal], 3),
    ];
    for (solid, expected) in cases {
        let world = floor_with(solid);
        let levels = world.levels([0, 0, 0], FaceDirection::PositiveY);
        assert_eq!(levels[3], expected, "cells {solid:?}");
    }
    // The corner on the far side of the face looks at none of these cells.
    let world = floor_with(&[side_along_u, side_along_v, diagonal]);
    assert_eq!(world.levels([0, 0, 0], FaceDirection::PositiveY)[0], 0);
}

/// A tunnel one cell wide in a wall, going down one cell for each cell along x and `height` cells tall: the cell at (x, y, 0) is air when `1 <= x + y <= height`.
fn staircase(height: i64) -> Cells {
    let mut cells = HashSet::new();
    for x in -12..=12 {
        for y in -14..=14 {
            for z in -2..=2 {
                if !(z == 0 && (1..=height).contains(&(x + y))) {
                    cells.insert([x, y, z]);
                }
            }
        }
    }
    Cells(cells)
}

#[test]
fn the_wall_of_a_three_tall_staircase_has_a_saddle_in_its_middle_row() {
    let world = staircase(3);
    // The wall cell at (x, y, -1) looks along +z at the air cell (x, y, 0); the face's axes are x then y, so its corners are lower left, lower right, upper left, upper
    // right.
    let rows = [
        (1, [3, 1, 1, 0]),
        // Two opposite corners are dark and the other two are not: no way of cutting the face in two triangles shades it right.
        (2, [1, 0, 0, 1]),
        (3, [0, 1, 1, 3]),
    ];
    for (row, expected) in rows {
        for x in -3..=3 {
            let voxel = [x, row - x, -1];
            assert_eq!(
                world.levels(voxel, FaceDirection::PositiveZ),
                expected,
                "wall at {voxel:?}"
            );
            let opposite = [x, row - x, 1];
            assert_eq!(
                world.levels(opposite, FaceDirection::NegativeZ),
                expected,
                "wall at {opposite:?}"
            );
        }
    }
}

#[test]
fn the_wall_of_a_two_tall_staircase_has_no_saddle() {
    let two = staircase(2);
    for x in -3..=3 {
        for row in [1, 2] {
            let levels = two.levels([x, row - x, -1], FaceDirection::PositiveZ);
            assert!(
                !(levels[0] == levels[3] && levels[1] == levels[2] && levels[0] != levels[1]),
                "row {row}: {levels:?}"
            );
        }
    }
}

#[test]
fn only_the_eight_cells_around_the_cell_a_face_looks_at_are_read() {
    let world = floor_with(&[[0, 1, 0], [3, 1, 3]]);
    for face in FaceDirection::ALL {
        let voxel = [0i64, 0, 0];
        let read = RefCell::new(HashSet::new());
        corner_occlusion(
            |cell| {
                read.borrow_mut().insert(cell);
                world.occludes(cell)
            },
            voxel,
            face,
        );
        let front = step(voxel, face);
        let (u_axis, v_axis) = quad_axes(face);
        let mut expected = HashSet::new();
        for du in -1..=1 {
            for dv in -1..=1 {
                if (du, dv) != (0, 0) {
                    let mut cell = front;
                    cell[u_axis] += du;
                    cell[v_axis] += dv;
                    expected.insert(cell);
                }
            }
        }
        assert_eq!(*read.borrow(), expected, "face {face:?}");
    }
}

#[test]
fn corner_levels_are_kept_in_two_bits_each_and_say_which_directions_they_are_constant_along() {
    let occlusion = CornerOcclusion::from_levels([3, 1, 0, 2]);
    assert_eq!(occlusion.levels(), [3, 1, 0, 2]);
    assert_eq!(CornerOcclusion::NONE.levels(), [0; 4]);
    assert_eq!(CornerOcclusion::default(), CornerOcclusion::NONE);
    // Constant along u: the two corners at each value of v agree. Along v: the two at each value of u.
    let cases = [
        ([0, 0, 0, 0], true, true),
        ([2, 2, 1, 1], true, false),
        ([2, 1, 2, 1], false, true),
        ([1, 0, 0, 1], false, false),
        ([3, 3, 3, 0], false, false),
    ];
    for (levels, along_u, along_v) in cases {
        let occlusion = CornerOcclusion::from_levels(levels);
        assert_eq!(occlusion.is_constant_along_u(), along_u, "{levels:?}");
        assert_eq!(occlusion.is_constant_along_v(), along_v, "{levels:?}");
    }
}

#[test]
#[should_panic(expected = "level")]
fn a_level_above_three_does_not_fit_and_panics() {
    CornerOcclusion::from_levels([0, 0, 4, 0]);
}

fn random_world() -> impl Strategy<Value = Cells> {
    (0u64..1000, 20u64..80).prop_map(|(seed, solid_per_hundred)| {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut cells = HashSet::new();
        for x in -3..=3 {
            for y in -3..=3 {
                for z in -3..=3 {
                    if next() % 100 < solid_per_hundred {
                        cells.insert([x, y, z]);
                    }
                }
            }
        }
        Cells(cells)
    })
}

proptest! {
    #[test]
    fn the_levels_equal_the_ones_worked_out_from_the_vertex_in_the_world(world in random_world()) {
        for (voxel, face) in world.drawn_faces() {
            prop_assert_eq!(world.levels(voxel, face), reference_levels(&world, voxel, face), "{:?} {:?}", voxel, face);
        }
    }

    #[test]
    fn faces_on_one_plane_that_share_a_vertex_agree_about_it(world in random_world()) {
        let mut seen: HashMap<(FaceDirection, [i64; 3]), (u8, [i64; 3])> = HashMap::new();
        for (voxel, face) in world.drawn_faces() {
            let levels = world.levels(voxel, face);
            for (corner, &corner_level) in levels.iter().enumerate() {
                let key = (face, vertex_position(voxel, face, corner));
                if let Some(&(level, other)) = seen.get(&key) {
                    prop_assert_eq!(level, corner_level, "faces at {:?} and {:?} disagree at {:?}", other, voxel, key.1);
                } else {
                    seen.insert(key, (corner_level, voxel));
                }
            }
        }
    }

    #[test]
    fn mirroring_the_world_mirrors_the_levels(world in random_world()) {
        // Mirroring along x turns a +x face into a -x one and swaps the corners along whichever of its axes is x.
        let mirrored = Cells(world.0.iter().map(|&[x, y, z]| [-x, y, z]).collect());
        for (voxel, face) in world.drawn_faces() {
            let levels = world.levels(voxel, face);
            let mirrored_voxel = [-voxel[0], voxel[1], voxel[2]];
            let mirrored_face = match face {
                FaceDirection::PositiveX => FaceDirection::NegativeX,
                FaceDirection::NegativeX => FaceDirection::PositiveX,
                other => other,
            };
            let got = mirrored.levels(mirrored_voxel, mirrored_face);
            let (u_axis, _) = quad_axes(face);
            // The corners are named by the face's own axes, which the mirror does not flip, so the corner that was at the high end of x is now at the low end.
            let flipped_corner = |corner: usize| {
                if face.axis() == 0 {
                    corner
                } else if u_axis == 0 {
                    corner ^ 1
                } else {
                    corner ^ 2
                }
            };
            for corner in 0..4 {
                prop_assert_eq!(got[flipped_corner(corner)], levels[corner], "{:?} {:?}", voxel, face);
            }
        }
    }
}
