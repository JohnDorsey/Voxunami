//! The corner levels of the quads a brick is meshed into (`Quad::occlusion`) are those of the world, whichever brick a face is in: a brick meshed with the twenty-six bricks that
//! touch it gets the same shade for a face at its edge as the world would give in one piece, and a quad that is a run of faces is shaded as each of its faces is.

use std::collections::HashSet;

use block_models::{BlockDefinition, BlockTable};
use proptest::prelude::*;
use voxel_grid::ChannelStorage;
use voxel_meshing::{
    BrickChannels, BrickNeighbourhood, CornerOcclusion, NEIGHBOUR_OFFSETS, Quad, corner_occlusion,
    mesh_brick, quad_axes,
};
use voxel_space::{BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, FaceDirection, LocalVoxelIndex};

const EDGE: i64 = BRICK_EDGE_LENGTH as i64;
const AIR: u32 = 0;
const STONE: u32 = 1;
const GLASS: u32 = 2;

fn table() -> BlockTable {
    let air = BlockDefinition::air();
    let stone = BlockDefinition::cube([0.5; 3]);
    let glass = BlockDefinition::cube([0.7; 3]).with_sight_through(true);
    BlockTable::from_definitions(&[Some(&air), Some(&stone), Some(&glass)])
}

/// The brick at `brick` (in bricks) filled from `world`, which is asked in voxels.
fn brick_of(world: &impl Fn([i64; 3]) -> u32, brick: [i64; 3]) -> ChannelStorage {
    let mut values = vec![AIR; BRICK_VOXEL_COUNT];
    for voxel in LocalVoxelIndex::all() {
        values[voxel.index()] = world([
            brick[0] * EDGE + i64::from(voxel.x()),
            brick[1] * EDGE + i64::from(voxel.y()),
            brick[2] * EDGE + i64::from(voxel.z()),
        ]);
    }
    ChannelStorage::from_values(&values)
}

/// The mesh of the brick at `brick` made from the twenty-six bricks that touch it, all cut from `world`.
fn quads_of_brick(world: &impl Fn([i64; 3]) -> u32, brick: [i64; 3]) -> Vec<Quad> {
    let center = brick_of(world, brick);
    let neighbours: Vec<ChannelStorage> = NEIGHBOUR_OFFSETS
        .iter()
        .map(|offset| {
            brick_of(
                world,
                std::array::from_fn(|axis| brick[axis] + i64::from(offset[axis])),
            )
        })
        .collect();
    let neighbourhood = BrickNeighbourhood {
        center: BrickChannels::blocks_only(&center),
        neighbours: std::array::from_fn(|slot| Some(BrickChannels::blocks_only(&neighbours[slot]))),
    };
    mesh_brick(&neighbourhood, &table()).quads
}

/// The place of voxel `(u, v)` of a quad of `face` at `origin`, in the world, `brick` being the brick's coordinate.
fn cell_of(quad: &Quad, brick: [i64; 3], along_u: u8, along_v: u8) -> [i64; 3] {
    let (u_axis, v_axis) = quad_axes(quad.face);
    let mut local = quad.origin.map(i64::from);
    local[u_axis] += i64::from(along_u);
    local[v_axis] += i64::from(along_v);
    std::array::from_fn(|axis| brick[axis] * EDGE + local[axis])
}

fn is_stone(world: &impl Fn([i64; 3]) -> u32, place: [i64; 3]) -> bool {
    world(place) == STONE
}

/// Every face of a stone cube whose front is not stone, in the brick at `brick`: what the whole faces of the mesh are.
fn exposed_faces(
    world: &impl Fn([i64; 3]) -> u32,
    brick: [i64; 3],
) -> HashSet<([i64; 3], FaceDirection)> {
    let mut faces = HashSet::new();
    for voxel in LocalVoxelIndex::all() {
        let place = [
            brick[0] * EDGE + i64::from(voxel.x()),
            brick[1] * EDGE + i64::from(voxel.y()),
            brick[2] * EDGE + i64::from(voxel.z()),
        ];
        if !is_stone(world, place) {
            continue;
        }
        for face in FaceDirection::ALL {
            let mut front = place;
            front[face.axis()] += if face.is_positive() { 1 } else { -1 };
            if !is_stone(world, front) {
                faces.insert((place, face));
            }
        }
    }
    faces
}

/// Checks the quads of a brick against the world: they cover exactly the exposed faces, once each, and every face of a quad has the corner levels the quad has.
fn check_brick(world: &impl Fn([i64; 3]) -> u32, brick: [i64; 3]) -> Result<(), TestCaseError> {
    let quads = quads_of_brick(world, brick);
    let mut covered = HashSet::new();
    for quad in &quads {
        for along_v in 0..quad.height {
            for along_u in 0..quad.width {
                let place = cell_of(quad, brick, along_u, along_v);
                prop_assert!(
                    covered.insert((place, quad.face)),
                    "a face is covered twice: {place:?} {:?}",
                    quad.face
                );
                let expected = corner_occlusion(|cell| is_stone(world, cell), place, quad.face);
                prop_assert_eq!(
                    quad.occlusion,
                    expected,
                    "face {:?} {:?} of the quad {:?}",
                    place,
                    quad.face,
                    quad
                );
            }
        }
    }
    prop_assert_eq!(covered, exposed_faces(world, brick));
    Ok(())
}

/// A hash of a place and a seed, told apart from the places beside it.
fn hash(seed: u64, place: [i64; 3]) -> u64 {
    let mut value = seed
        ^ (place[0] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (place[1] as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (place[2] as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    value ^= value >> 29;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^ (value >> 32)
}

fn random_world(seed: u64, stone_per_mille: u64) -> impl Fn([i64; 3]) -> u32 {
    move |place| match hash(seed, place) % 1000 {
        roll if roll < stone_per_mille => STONE,
        roll if roll < stone_per_mille + 60 => GLASS,
        _ => AIR,
    }
}

#[test]
fn a_block_on_a_floor_shades_the_corners_of_the_floor_beside_it_and_a_run_of_floor_beside_a_wall_is_one_shaded_quad()
 {
    // Stone up to y = 15, and a wall of stone one voxel thick along z at x = 10, from y = 16 to 17, as far as the eye can see.
    let world = |[x, y, _z]: [i64; 3]| {
        if y < 16 || (x == 10 && y < 18) {
            STONE
        } else {
            AIR
        }
    };
    let quads = quads_of_brick(&world, [0, 0, 0]);
    let tops: Vec<&Quad> = quads
        .iter()
        .filter(|quad| quad.face == FaceDirection::PositiveY && quad.origin[1] == 15)
        .collect();
    // A top face spans z (u) and x (v). The floor beside the wall on its low-x side is one quad along the whole wall, one voxel across, shaded on its high-x corners by the wall beside it and by the wall diagonal to them (level 2).
    let beside = tops
        .iter()
        .find(|quad| quad.origin == [9, 15, 0])
        .expect("a quad of the floor beside the wall");
    assert_eq!((beside.width, beside.height), (32, 1));
    assert_eq!(beside.occlusion, CornerOcclusion::from_levels([0, 0, 2, 2]));
    // On the other side the corners at low x are shaded the same.
    let other_side = tops
        .iter()
        .find(|quad| quad.origin == [11, 15, 0])
        .expect("a quad of the floor on the other side");
    assert_eq!((other_side.width, other_side.height), (32, 1));
    assert_eq!(
        other_side.occlusion,
        CornerOcclusion::from_levels([2, 2, 0, 0])
    );
}

#[test]
fn a_flat_floor_in_the_open_is_one_quad_with_no_shade() {
    let world = |[_, y, _]: [i64; 3]| if y < 16 { STONE } else { AIR };
    let quads = quads_of_brick(&world, [0, 0, 0]);
    let tops: Vec<&Quad> = quads
        .iter()
        .filter(|quad| quad.face == FaceDirection::PositiveY)
        .collect();
    assert_eq!(tops.len(), 1);
    assert_eq!((tops[0].width, tops[0].height), (32, 32));
    assert_eq!(tops[0].occlusion, CornerOcclusion::NONE);
}

#[test]
fn a_block_placed_on_a_floor_shades_one_corner_of_each_diagonal_floor_voxel_and_the_two_corners_of_each_side_one()
 {
    let world = |[x, y, z]: [i64; 3]| {
        if y < 16 || [x, y, z] == [10, 16, 10] {
            STONE
        } else {
            AIR
        }
    };
    let quads = quads_of_brick(&world, [0, 0, 0]);
    let top_over = |x: i64, z: i64| {
        quads
            .iter()
            .find(|quad| {
                quad.face == FaceDirection::PositiveY
                    && quad.origin[1] == 15
                    && (i64::from(quad.origin[2])
                        ..i64::from(quad.origin[2]) + i64::from(quad.width))
                        .contains(&z)
                    && (i64::from(quad.origin[0])
                        ..i64::from(quad.origin[0]) + i64::from(quad.height))
                        .contains(&x)
            })
            .unwrap_or_else(|| panic!("nothing covers {x} {z}"))
    };
    // A top face spans z (u) and x (v); corner = u + 2 v.
    // Each of the four diagonal voxels is shaded at the one corner that touches the block, and over the whole of it alike, wherever the triangles are cut.
    assert_eq!(
        top_over(9, 9).occlusion,
        CornerOcclusion::from_levels([0, 0, 0, 1])
    );
    assert_eq!(
        top_over(11, 9).occlusion,
        CornerOcclusion::from_levels([0, 1, 0, 0])
    );
    assert_eq!(
        top_over(9, 11).occlusion,
        CornerOcclusion::from_levels([0, 0, 1, 0])
    );
    assert_eq!(
        top_over(11, 11).occlusion,
        CornerOcclusion::from_levels([1, 0, 0, 0])
    );
    // The four beside it are shaded at the two corners of the edge they share with it.
    assert_eq!(
        top_over(9, 10).occlusion,
        CornerOcclusion::from_levels([0, 0, 1, 1])
    );
    assert_eq!(
        top_over(11, 10).occlusion,
        CornerOcclusion::from_levels([1, 1, 0, 0])
    );
    assert_eq!(
        top_over(10, 9).occlusion,
        CornerOcclusion::from_levels([0, 1, 0, 1])
    );
    assert_eq!(
        top_over(10, 11).occlusion,
        CornerOcclusion::from_levels([1, 0, 1, 0])
    );
    // The top of the block itself is in the open.
    let block_top = quads
        .iter()
        .find(|quad| quad.face == FaceDirection::PositiveY && quad.origin[1] == 16)
        .expect("the top of the block");
    assert_eq!(block_top.occlusion, CornerOcclusion::NONE);
}

#[test]
fn a_face_at_the_edge_of_a_brick_is_shaded_by_a_block_in_the_brick_that_touches_it_only_at_a_corner()
 {
    // A floor with one block on it just across the corner of the brick (in the brick at x = 1, z = 1, which touches the middle brick along an edge of the floor's top layer
    // only at that corner), diagonal to the last voxel of the floor of the middle brick.
    let world = |[x, y, z]: [i64; 3]| {
        if y < 16 || [x, y, z] == [32, 16, 32] {
            STONE
        } else {
            AIR
        }
    };
    let quads = quads_of_brick(&world, [0, 0, 0]);
    let last = quads
        .iter()
        .find(|quad| {
            quad.face == FaceDirection::PositiveY
                && quad.origin[1] == 15
                && quad.origin[0] + quad.height == 32
                && quad.origin[2] + quad.width == 32
        })
        .expect("the quad at the high corner of the floor");
    assert_eq!((last.width, last.height), (1, 1));
    assert_eq!(last.occlusion, CornerOcclusion::from_levels([0, 0, 0, 1]));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, ..ProptestConfig::default() })]

    /// Whatever the world, every brick of a block of bricks, each meshed from the twenty-six bricks that touch it, has quads that cover the exposed faces once and shade
    /// every face of a quad as the world shades that face.
    #[test]
    fn bricks_meshed_in_pieces_shade_every_face_as_the_world_does_in_one(
        seed in any::<u64>(),
        stone_per_mille in 100u64..900,
    ) {
        let world = random_world(seed, stone_per_mille);
        for x in 0..2 {
            for y in 0..2 {
                for z in 0..2 {
                    check_brick(&world, [x, y, z])?;
                }
            }
        }
    }

    /// The same in a world made of runs, which the greedy merge joins into quads that are many voxels across (the random worlds have few runs).
    #[test]
    fn quads_that_are_runs_of_faces_are_shaded_as_each_face_of_the_run_is(
        seed in any::<u64>(),
        column in 2u64..12,
    ) {
        let world = move |place: [i64; 3]| {
            let height = 10 + (hash(seed, [place[0] / column as i64, 0, place[2] / column as i64]) % 12) as i64;
            if place[1] < height { STONE } else { AIR }
        };
        check_brick(&world, [0, 0, 0])?;
        check_brick(&world, [1, 0, 0])?;
    }
}
