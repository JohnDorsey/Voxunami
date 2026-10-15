use voxel_grid::ChannelStorage;
use voxel_space::{BRICK_EDGE_LENGTH, FaceDirection, LocalVoxelIndex};

use crate::quad::{Quad, quad_axes};

const EDGE: usize = BRICK_EDGE_LENGTH;

/// The voxels a brick's mesh depends on: its own, and the six bricks that touch its faces (only
/// the layer next to the brick matters).
///
/// `neighbours` is indexed by [`FaceDirection::index`]. A missing neighbour is treated as empty,
/// so faces next to it are drawn; callers that stream bricks should mesh only once the
/// neighbours exist, or remesh when they arrive.
#[derive(Clone, Copy, Debug)]
pub struct BrickNeighbourhood<'a> {
    pub center: &'a ChannelStorage,
    pub neighbours: [Option<&'a ChannelStorage>; 6],
}

/// Greedy-meshed quads for every exposed face of `input.center`. `is_opaque` says which voxel
/// values hide the faces behind them.
pub fn mesh_brick(input: &BrickNeighbourhood<'_>, is_opaque: impl Fn(u32) -> bool) -> Vec<Quad> {
    // Fast paths for the commonest bricks: sky, and solid ground with solid ground all around.
    if let Some(value) = input.center.uniform_value() {
        if !is_opaque(value) {
            return Vec::new();
        }
        let fully_buried = input.neighbours.iter().all(|neighbour| {
            neighbour
                .and_then(ChannelStorage::uniform_value)
                .is_some_and(&is_opaque)
        });
        if fully_buried {
            return Vec::new();
        }
    }

    let values: Vec<u32> = input.center.iter().collect();
    let opaque: Vec<bool> = values.iter().map(|&value| is_opaque(value)).collect();
    let mut quads = Vec::new();
    for face in FaceDirection::ALL {
        let layer_behind = neighbour_layer(
            input.neighbours[usize::from(face.index())],
            face,
            &is_opaque,
        );
        mesh_face(face, &values, &opaque, &layer_behind, &mut quads);
    }
    quads
}

/// Which voxels of the neighbour's layer touching `face` are opaque, indexed `v * 32 + u` in the
/// quad axes of `face`.
fn neighbour_layer(
    neighbour: Option<&ChannelStorage>,
    face: FaceDirection,
    is_opaque: &impl Fn(u32) -> bool,
) -> Vec<bool> {
    let Some(neighbour) = neighbour else {
        return vec![false; EDGE * EDGE];
    };
    let (u_axis, v_axis) = quad_axes(face);
    let touching_slice = if face.is_positive() { 0 } else { EDGE - 1 };
    let mut layer = vec![false; EDGE * EDGE];
    for v in 0..EDGE {
        for u in 0..EDGE {
            let mut p = [0usize; 3];
            p[face.axis()] = touching_slice;
            p[u_axis] = u;
            p[v_axis] = v;
            layer[v * EDGE + u] = is_opaque(neighbour.get(local(p)));
        }
    }
    layer
}

fn local(p: [usize; 3]) -> LocalVoxelIndex {
    LocalVoxelIndex::new(p[0] as u8, p[1] as u8, p[2] as u8)
}

fn mesh_face(
    face: FaceDirection,
    values: &[u32],
    opaque: &[bool],
    layer_behind: &[bool],
    quads: &mut Vec<Quad>,
) {
    let axis = face.axis();
    let (u_axis, v_axis) = quad_axes(face);
    let mut mask: Vec<Option<u32>> = vec![None; EDGE * EDGE];
    for slice in 0..EDGE {
        let mut any = false;
        for v in 0..EDGE {
            for u in 0..EDGE {
                let mut p = [0usize; 3];
                p[axis] = slice;
                p[u_axis] = u;
                p[v_axis] = v;
                let index = local(p).index();
                let exposed = opaque[index]
                    && match (face.is_positive(), slice) {
                        (true, last) if last == EDGE - 1 => !layer_behind[v * EDGE + u],
                        (false, 0) => !layer_behind[v * EDGE + u],
                        (positive, _) => {
                            let mut next = p;
                            next[axis] = if positive { slice + 1 } else { slice - 1 };
                            !opaque[local(next).index()]
                        }
                    };
                mask[v * EDGE + u] = exposed.then_some(values[index]);
                any |= exposed;
            }
        }
        if any {
            merge_slice(face, slice, &mut mask, quads);
        }
    }
}

/// Covers the set cells of `mask` with maximal rectangles of equal material, clearing it as it goes.
fn merge_slice(face: FaceDirection, slice: usize, mask: &mut [Option<u32>], quads: &mut Vec<Quad>) {
    let axis = face.axis();
    let (u_axis, v_axis) = quad_axes(face);
    for v0 in 0..EDGE {
        let mut u0 = 0;
        while u0 < EDGE {
            let Some(material) = mask[v0 * EDGE + u0] else {
                u0 += 1;
                continue;
            };
            let mut width = 1;
            while u0 + width < EDGE && mask[v0 * EDGE + u0 + width] == Some(material) {
                width += 1;
            }
            let mut height = 1;
            'grow: while v0 + height < EDGE {
                for u in u0..u0 + width {
                    if mask[(v0 + height) * EDGE + u] != Some(material) {
                        break 'grow;
                    }
                }
                height += 1;
            }
            for v in v0..v0 + height {
                for u in u0..u0 + width {
                    mask[v * EDGE + u] = None;
                }
            }
            let mut origin = [0u8; 3];
            origin[axis] = slice as u8;
            origin[u_axis] = u0 as u8;
            origin[v_axis] = v0 as u8;
            quads.push(Quad {
                origin,
                width: width as u8,
                height: height as u8,
                face,
                material,
            });
            u0 += width;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;
    use voxel_space::BRICK_VOXEL_COUNT;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const DIRT: u32 = 2;

    fn opaque(value: u32) -> bool {
        value != AIR
    }

    fn brick_from(fill: impl Fn(usize, usize, usize) -> u32) -> ChannelStorage {
        let mut values = vec![AIR; BRICK_VOXEL_COUNT];
        for z in 0..EDGE {
            for y in 0..EDGE {
                for x in 0..EDGE {
                    values[local([x, y, z]).index()] = fill(x, y, z);
                }
            }
        }
        ChannelStorage::from_values(&values)
    }

    fn isolated(center: &ChannelStorage) -> Vec<Quad> {
        mesh_brick(
            &BrickNeighbourhood {
                center,
                neighbours: [None; 6],
            },
            opaque,
        )
    }

    fn quad(origin: [u8; 3], width: u8, height: u8, face: FaceDirection, material: u32) -> Quad {
        Quad {
            origin,
            width,
            height,
            face,
            material,
        }
    }

    #[test]
    fn a_lone_voxel_gets_one_unit_quad_per_face() {
        let center = brick_from(|x, y, z| if (x, y, z) == (5, 6, 7) { STONE } else { AIR });
        let quads = isolated(&center);
        assert_eq!(quads.len(), 6);
        for face in FaceDirection::ALL {
            assert!(
                quads.contains(&quad([5, 6, 7], 1, 1, face, STONE)),
                "{face:?}"
            );
        }
    }

    #[test]
    fn a_full_brick_in_the_open_is_six_big_quads() {
        let quads = isolated(&ChannelStorage::uniform(STONE));
        assert_eq!(quads.len(), 6);
        for face in FaceDirection::ALL {
            // The voxel that owns a positive face is the last one along its axis.
            let mut origin = [0, 0, 0];
            if face.is_positive() {
                origin[face.axis()] = 31;
            }
            assert!(
                quads.contains(&quad(origin, 32, 32, face, STONE)),
                "{face:?}"
            );
        }
    }

    #[test]
    fn a_solid_neighbour_hides_the_faces_that_touch_it() {
        let center = ChannelStorage::uniform(STONE);
        let wall = ChannelStorage::uniform(DIRT);
        let mut neighbours = [None; 6];
        neighbours[usize::from(FaceDirection::PositiveX.index())] = Some(&wall);
        let quads = mesh_brick(
            &BrickNeighbourhood {
                center: &center,
                neighbours,
            },
            opaque,
        );
        assert_eq!(quads.len(), 5);
        assert!(
            quads
                .iter()
                .all(|quad| quad.face != FaceDirection::PositiveX)
        );
    }

    #[test]
    fn only_the_neighbours_touching_layer_matters() {
        // A neighbour that is solid everywhere except the layer touching the brick.
        let center = ChannelStorage::uniform(STONE);
        let hollow_touching_layer = brick_from(|x, _, _| if x == 0 { AIR } else { STONE });
        let mut neighbours = [None; 6];
        neighbours[usize::from(FaceDirection::PositiveX.index())] = Some(&hollow_touching_layer);
        let quads = mesh_brick(
            &BrickNeighbourhood {
                center: &center,
                neighbours,
            },
            opaque,
        );
        assert!(
            quads
                .iter()
                .any(|quad| quad.face == FaceDirection::PositiveX)
        );

        // Same neighbour seen from the other side: its x = 31 layer is solid, so no face.
        let mut neighbours = [None; 6];
        neighbours[usize::from(FaceDirection::NegativeX.index())] = Some(&hollow_touching_layer);
        let quads = mesh_brick(
            &BrickNeighbourhood {
                center: &center,
                neighbours,
            },
            opaque,
        );
        assert!(
            quads
                .iter()
                .all(|quad| quad.face != FaceDirection::NegativeX)
        );
    }

    #[test]
    fn a_flat_floor_is_one_quad_on_top_and_one_on_each_open_side() {
        let center = brick_from(|_, y, _| if y < 16 { STONE } else { AIR });
        let quads = isolated(&center);
        assert_eq!(quads.len(), 6);
        assert!(quads.contains(&quad([0, 15, 0], 32, 32, FaceDirection::PositiveY, STONE)));
        assert!(quads.contains(&quad([0, 0, 0], 32, 32, FaceDirection::NegativeY, STONE)));
        // Side walls are 16 voxels tall: for x faces, u = y (height 16) and v = z (32).
        assert!(quads.contains(&quad([0, 0, 0], 16, 32, FaceDirection::NegativeX, STONE)));
    }

    #[test]
    fn different_materials_never_merge() {
        let center = brick_from(|x, y, _| {
            if y == 0 {
                if x < 10 { STONE } else { DIRT }
            } else {
                AIR
            }
        });
        let tops: Vec<Quad> = isolated(&center)
            .into_iter()
            .filter(|quad| quad.face == FaceDirection::PositiveY)
            .collect();
        assert_eq!(tops.len(), 2);
        // Y faces span z (width) then x (height): stone covers x 0..10, dirt x 10..32.
        assert!(tops.contains(&quad([0, 0, 0], 32, 10, FaceDirection::PositiveY, STONE)));
        assert!(tops.contains(&quad([10, 0, 0], 32, 22, FaceDirection::PositiveY, DIRT)));
    }

    #[test]
    fn empty_and_buried_bricks_take_the_fast_path_and_produce_nothing() {
        assert!(isolated(&ChannelStorage::uniform(AIR)).is_empty());
        let stone = ChannelStorage::uniform(STONE);
        let dirt = ChannelStorage::uniform(DIRT);
        let buried = BrickNeighbourhood {
            center: &stone,
            neighbours: [Some(&dirt); 6],
        };
        assert!(mesh_brick(&buried, opaque).is_empty());
        // One missing neighbour exposes a face.
        let mut neighbours = [Some(&dirt); 6];
        neighbours[3] = None;
        assert_eq!(
            mesh_brick(
                &BrickNeighbourhood {
                    center: &stone,
                    neighbours
                },
                opaque
            )
            .len(),
            1
        );
    }

    /// A deterministic pseudo-random brick: the proptest shrinker only needs to shrink the seed.
    fn random_brick(seed: u64, solid_per_mille: u64, materials: u64) -> Vec<u32> {
        let mut state = seed | 1;
        (0..BRICK_VOXEL_COUNT)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if state % 1000 < solid_per_mille {
                    1 + (state >> 20) as u32 % materials as u32
                } else {
                    AIR
                }
            })
            .collect()
    }

    fn brick_model() -> impl Strategy<Value = Vec<u32>> {
        prop_oneof![
            1 => prop_oneof![Just(AIR), Just(STONE), Just(DIRT)].prop_map(|value| vec![value; BRICK_VOXEL_COUNT]),
            4 => (any::<u64>(), prop_oneof![Just(20u64), Just(500), Just(980)], 1u64..4)
                .prop_map(|(seed, density, materials)| random_brick(seed, density, materials)),
        ]
    }

    /// Every exposed face, found the slow obvious way: look at the voxel next door in world terms.
    fn reference_faces(
        center: &[u32],
        neighbours: &[Option<Vec<u32>>; 6],
    ) -> HashMap<([usize; 3], FaceDirection), u32> {
        let at = |p: [i64; 3]| -> u32 {
            let outside: Vec<usize> = (0..3)
                .filter(|&a| !(0..EDGE as i64).contains(&p[a]))
                .collect();
            match outside[..] {
                [] => center[local([p[0] as usize, p[1] as usize, p[2] as usize]).index()],
                [axis] => {
                    let face =
                        FaceDirection::from_index((2 * axis + usize::from(p[axis] >= 0)) as u8)
                            .unwrap();
                    let mut wrapped = p;
                    wrapped[axis] = p[axis].rem_euclid(EDGE as i64);
                    match &neighbours[usize::from(face.index())] {
                        Some(values) => {
                            values[local([
                                wrapped[0] as usize,
                                wrapped[1] as usize,
                                wrapped[2] as usize,
                            ])
                            .index()]
                        }
                        None => AIR,
                    }
                }
                _ => unreachable!("faces only look one voxel along one axis"),
            }
        };
        let mut faces = HashMap::new();
        for z in 0..EDGE {
            for y in 0..EDGE {
                for x in 0..EDGE {
                    let value = center[local([x, y, z]).index()];
                    if !opaque(value) {
                        continue;
                    }
                    for face in FaceDirection::ALL {
                        let [dx, dy, dz] = face.offset();
                        let next = [x as i64 + dx, y as i64 + dy, z as i64 + dz];
                        if !opaque(at(next)) {
                            faces.insert(([x, y, z], face), value);
                        }
                    }
                }
            }
        }
        faces
    }

    fn expand(quads: &[Quad]) -> (HashMap<([usize; 3], FaceDirection), u32>, usize) {
        let mut faces = HashMap::new();
        let mut total = 0;
        for quad in quads {
            let (u_axis, v_axis) = quad_axes(quad.face);
            for j in 0..usize::from(quad.height) {
                for i in 0..usize::from(quad.width) {
                    let mut p = [
                        usize::from(quad.origin[0]),
                        usize::from(quad.origin[1]),
                        usize::from(quad.origin[2]),
                    ];
                    p[u_axis] += i;
                    p[v_axis] += j;
                    faces.insert((p, quad.face), quad.material);
                    total += 1;
                }
            }
        }
        (faces, total)
    }

    proptest! {
        /// The meshed quads must cover exactly the exposed faces: no face missing, none invented,
        /// none covered twice, and every quad carries its voxels' material.
        #[test]
        fn quads_cover_exactly_the_exposed_faces(
            center in brick_model(),
            neighbours in proptest::array::uniform6(prop_oneof![1 => Just(None), 3 => brick_model().prop_map(Some)]),
        ) {
            let center_storage = ChannelStorage::from_values(&center);
            let neighbour_storages: Vec<Option<ChannelStorage>> = neighbours
                .iter()
                .map(|values| values.as_ref().map(|values| ChannelStorage::from_values(values)))
                .collect();
            let input = BrickNeighbourhood {
                center: &center_storage,
                neighbours: std::array::from_fn(|index| neighbour_storages[index].as_ref()),
            };

            let quads = mesh_brick(&input, opaque);
            let (covered, covered_count) = expand(&quads);
            let expected = reference_faces(&center, &neighbours);

            prop_assert_eq!(covered_count, covered.len(), "a face is covered by two quads");
            prop_assert_eq!(covered.len(), expected.len());
            prop_assert!(covered == expected, "covered faces differ from the exposed faces");
        }

        #[test]
        fn greedy_merging_never_uses_more_quads_than_exposed_faces(center in brick_model()) {
            let storage = ChannelStorage::from_values(&center);
            let quads = isolated(&storage);
            let exposed = reference_faces(&center, &Default::default()).len();
            prop_assert!(quads.len() <= exposed);
        }
    }
}
