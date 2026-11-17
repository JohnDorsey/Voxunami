use std::collections::HashSet;

use voxel_grid::ChannelStorage;
use voxel_space::{BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, FaceDirection, LocalVoxelIndex};

use super::*;

const EDGE: i32 = BRICK_EDGE_LENGTH as i32;

/// What the world holds at a place, told apart from its neighbours' by a hash, so that a lookup that lands one voxel or one brick away is caught.
fn world_block(x: i64, y: i64, z: i64) -> u32 {
    let mut hash = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    (hash >> 40) as u32
}

/// The brick at `offset` from the middle one, filled from [`world_block`].
fn brick_of_world(offset: [i8; 3]) -> ChannelStorage {
    let mut values = vec![0; BRICK_VOXEL_COUNT];
    for voxel in LocalVoxelIndex::all() {
        let place =
            |axis: usize, local: u8| i64::from(offset[axis]) * i64::from(EDGE) + i64::from(local);
        values[voxel.index()] = world_block(
            place(0, voxel.x()),
            place(1, voxel.y()),
            place(2, voxel.z()),
        );
    }
    ChannelStorage::from_values(&values)
}

/// All twenty-seven bricks, the middle one at `NEIGHBOUR_OFFSETS.len()` in a list of its own.
struct Around {
    center: ChannelStorage,
    neighbours: Vec<ChannelStorage>,
}

impl Around {
    fn new() -> Self {
        Self {
            center: brick_of_world([0, 0, 0]),
            neighbours: NEIGHBOUR_OFFSETS
                .iter()
                .map(|&offset| brick_of_world(offset))
                .collect(),
        }
    }

    /// The neighbourhood with the bricks whose offsets `present` says are there.
    fn neighbourhood(&self, present: impl Fn([i8; 3]) -> bool) -> BrickNeighbourhood<'_> {
        BrickNeighbourhood {
            center: BrickChannels::blocks_only(&self.center),
            neighbours: std::array::from_fn(|slot| {
                present(NEIGHBOUR_OFFSETS[slot])
                    .then(|| BrickChannels::blocks_only(&self.neighbours[slot]))
            }),
        }
    }
}

#[test]
fn the_offsets_are_the_twenty_six_bricks_around_a_brick_faces_then_edges_then_corners() {
    assert_eq!(NEIGHBOUR_OFFSETS.len(), 26);
    let distinct: HashSet<[i8; 3]> = NEIGHBOUR_OFFSETS.iter().copied().collect();
    assert_eq!(distinct.len(), 26);
    for offset in NEIGHBOUR_OFFSETS {
        assert!(offset.iter().all(|component| (-1..=1).contains(component)));
        assert_ne!(offset, [0, 0, 0]);
    }
    let touching = |offset: &[i8; 3]| offset.iter().filter(|&&component| component != 0).count();
    let counts: Vec<usize> = NEIGHBOUR_OFFSETS.iter().map(touching).collect();
    assert_eq!(counts[..6], [1; 6], "faces first");
    assert_eq!(counts[6..18], [2; 12], "then edges");
    assert_eq!(counts[18..], [3; 8], "then corners");
}

#[test]
fn the_first_six_offsets_are_the_faces_in_the_order_of_their_indices() {
    // Code that indexes the neighbours by face keeps working because of this.
    for face in FaceDirection::ALL {
        let [x, y, z] = face.offset();
        assert_eq!(
            NEIGHBOUR_OFFSETS[usize::from(face.index())],
            [x as i8, y as i8, z as i8],
            "{face:?}"
        );
    }
}

#[test]
fn a_slot_is_found_from_its_offset_and_only_for_a_brick_that_touches() {
    for (slot, &offset) in NEIGHBOUR_OFFSETS.iter().enumerate() {
        assert_eq!(neighbour_slot(offset), Some(slot));
    }
    assert_eq!(neighbour_slot([0, 0, 0]), None);
    assert_eq!(neighbour_slot([2, 0, 0]), None);
    assert_eq!(neighbour_slot([0, -2, 1]), None);
}

#[test]
fn a_neighbourhood_of_faces_has_no_edge_or_corner_bricks() {
    let around = Around::new();
    let faces: [Option<BrickChannels<'_>>; 6] =
        std::array::from_fn(|face| Some(BrickChannels::blocks_only(&around.neighbours[face])));
    let neighbourhood =
        BrickNeighbourhood::with_face_neighbours(BrickChannels::blocks_only(&around.center), faces);
    assert!(neighbourhood.neighbours[..6].iter().all(Option::is_some));
    assert!(neighbourhood.neighbours[6..].iter().all(Option::is_none));
}

#[test]
fn every_voxel_of_the_brick_and_of_the_layer_round_it_is_found_where_the_world_has_it() {
    let around = Around::new();
    let neighbourhood = around.neighbourhood(|_| true);
    for x in -1..=EDGE {
        for y in -1..=EDGE {
            for z in -1..=EDGE {
                let (channels, voxel) = neighbourhood
                    .voxel_at([x, y, z])
                    .unwrap_or_else(|| panic!("nothing found at {:?}", [x, y, z]));
                assert_eq!(
                    channels.blocks.get(voxel),
                    world_block(i64::from(x), i64::from(y), i64::from(z)),
                    "at {:?}",
                    [x, y, z]
                );
            }
        }
    }
}

#[test]
fn a_voxel_in_a_missing_brick_is_not_found_and_every_other_is() {
    let around = Around::new();
    // Lose one brick of each kind: a face, an edge and a corner.
    for missing in [[0, 1, 0], [1, 0, -1], [-1, 1, 1]] {
        let neighbourhood = around.neighbourhood(|offset| offset != missing);
        for x in -1..=EDGE {
            for y in -1..=EDGE {
                for z in -1..=EDGE {
                    let cell = [x, y, z];
                    let component = |value: i32| match value {
                        ..0 => -1,
                        0..EDGE => 0,
                        _ => 1,
                    };
                    let in_missing = cell.map(component) == missing;
                    assert_eq!(
                        neighbourhood.voxel_at(cell).is_none(),
                        in_missing,
                        "brick {missing:?} missing, voxel {cell:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_voxels_of_the_brick_itself_are_found_without_any_neighbour() {
    let around = Around::new();
    let alone = around.neighbourhood(|_| false);
    for x in 0..EDGE {
        assert!(alone.voxel_at([x, 5, 31 - x]).is_some());
    }
    assert!(alone.voxel_at([-1, 5, 5]).is_none());
    assert!(alone.voxel_at([5, 5, EDGE]).is_none());
}

#[test]
#[should_panic(expected = "outside")]
fn a_voxel_more_than_one_past_the_brick_is_a_mistake_and_panics() {
    let around = Around::new();
    around.neighbourhood(|_| true).voxel_at([-2, 0, 0]);
}

#[test]
#[should_panic(expected = "outside")]
fn a_voxel_beyond_the_high_side_by_more_than_one_panics() {
    let around = Around::new();
    around.neighbourhood(|_| true).voxel_at([0, EDGE + 1, 0]);
}
