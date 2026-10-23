//! The coarse cells of a brick: what each of its voxels is drawn as from far away (`docs/lod-spec.md`, 38d).
//!
//! This is the other half of meshing: the mesher decides which faces of a brick are drawn, and here the same decision says how much surface each voxel
//! shows to the air, which is what a coarse cell chooses its block by (`coarse_cells::combine`).

use block_models::{BlockTable, ModelId, SUBDIVISIONS};
use coarse_cells::{CELLS_PER_NODE, CoarseCell, CoarseNode, FULL_LIGHT};
use voxel_space::{BRICK_EDGE_LENGTH, FaceDirection, LocalVoxelIndex};

use crate::mesher::{BrickChannels, BrickNeighbourhood, uniform_model};

const EDGE: usize = BRICK_EDGE_LENGTH;

/// Sub-faces in a face of a voxel (it is cut into `SUBDIVISIONS` by `SUBDIVISIONS`).
const SUB_FACES: u32 = (SUBDIVISIONS * SUBDIVISIONS) as u32;

/// Exposure of one sub-face: a whole face is [`CoarseCell::EXPOSURE_ONE`].
const EXPOSURE_PER_SUB_FACE: u32 = CoarseCell::EXPOSURE_ONE / SUB_FACES;

/// The cells of a brick, one for each voxel, at detail level 0 (the voxels of the brick are the cells of the node).
///
/// For a voxel:
/// - *fill* is how much of the voxel its shape fills, 0 to 255 (a whole block 255, a slab 128; anything at all is at least 1);
/// - *block* is its block, or `air` for an empty one;
/// - *exposure* is the area of its surface that the mesher would draw, in whole faces: the faces of its shape on the boundary of the voxel that what is
///   next to it does not hide, and the faces inside it (the top of a slab). A whole block with one face to the air has [`CoarseCell::EXPOSURE_ONE`];
/// - *light* is the voxel's own for an empty one, and for a filled one the brightest light of the places its exposed faces look out at (its own where
///   none is exposed). The light is the brightness the mesher is given, 0 to 255 ([`FULL_LIGHT`] when the brick has no light channel).
///
/// Next to the brick, `input.neighbours` says what is there; a missing neighbour is air, so faces next to it are exposed (as meshing treats it).
pub fn coarse_node_of_brick(
    input: &BrickNeighbourhood<'_>,
    table: &BlockTable,
    air: u32,
) -> CoarseNode {
    if let Some(node) = uniform_node(input, table, air) {
        return node;
    }
    let center = &input.center;
    let models: Vec<ModelId> = LocalVoxelIndex::all()
        .map(|voxel| model_at(center, table, voxel))
        .collect();
    let lights: Vec<u8> = LocalVoxelIndex::all()
        .map(|voxel| center.light_at(voxel))
        .collect();
    let blocks: Vec<u32> = center.blocks.iter().collect();
    let mut cells = Vec::with_capacity(CELLS_PER_NODE);
    for z in 0..EDGE {
        for y in 0..EDGE {
            for x in 0..EDGE {
                let index = LocalVoxelIndex::new(x as u8, y as u8, z as u8).index();
                let shape = table.shape(models[index]);
                let fill = fill_of(shape.filled_cells());
                if fill == 0 {
                    cells.push(CoarseCell::empty(air, lights[index]));
                    continue;
                }
                let mut exposure = 0u32;
                let mut light = None::<u8>;
                for face in FaceDirection::ALL {
                    let rectangles = shape.exposed(face);
                    if rectangles.is_empty() {
                        continue;
                    }
                    // What lies against this face of the voxel: its model and brightness, if it is known.
                    let front = in_front(input, table, &models, &lights, [x, y, z], face);
                    for rectangle in rectangles {
                        let (visible, brightness) = if rectangle.on_boundary(face) {
                            let hidden = front.map_or(0, |(model, _)| {
                                table.hidden_by_neighbour(models[index], face, model)
                            });
                            (
                                rectangle.mask() & !hidden,
                                front.map_or(lights[index], |(_, light)| light),
                            )
                        } else {
                            (rectangle.mask(), lights[index])
                        };
                        let sub_faces = visible.count_ones();
                        if sub_faces > 0 {
                            exposure += sub_faces * EXPOSURE_PER_SUB_FACE;
                            light = Some(light.map_or(brightness, |found| found.max(brightness)));
                        }
                    }
                }
                cells.push(CoarseCell {
                    block: blocks[index],
                    fill,
                    light: light.unwrap_or(lights[index]),
                    exposure,
                });
            }
        }
    }
    CoarseNode::from_cells(cells)
}

/// How much of a voxel a shape fills, 0 to 255: of the 512 sub-cells it is cut into, rounded up so that anything at all is at least 1.
fn fill_of(filled_sub_cells: u32) -> u8 {
    let total = (SUBDIVISIONS * SUBDIVISIONS * SUBDIVISIONS) as u32;
    (filled_sub_cells * 255).div_ceil(total) as u8
}

fn model_at(channels: &BrickChannels<'_>, table: &BlockTable, voxel: LocalVoxelIndex) -> ModelId {
    table.model_of(
        channels.blocks.get(voxel),
        channels.state_at(voxel),
        channels.orientation_at(voxel),
    )
}

/// The model and brightness of what is against `face` of the voxel at `at`, inside the brick or in the neighbour that touches it; `None` if that
/// neighbour is not there.
fn in_front(
    input: &BrickNeighbourhood<'_>,
    table: &BlockTable,
    models: &[ModelId],
    lights: &[u8],
    at: [usize; 3],
    face: FaceDirection,
) -> Option<(ModelId, u8)> {
    let step = face.offset();
    let next = [0, 1, 2].map(|axis| at[axis] as i64 + step[axis]);
    if next
        .iter()
        .all(|&component| (0..EDGE as i64).contains(&component))
    {
        let index = LocalVoxelIndex::new(next[0] as u8, next[1] as u8, next[2] as u8).index();
        return Some((models[index], lights[index]));
    }
    let neighbour = input.neighbours[usize::from(face.index())].as_ref()?;
    let wrapped = next.map(|component| component.rem_euclid(EDGE as i64) as u8);
    let voxel = LocalVoxelIndex::new(wrapped[0], wrapped[1], wrapped[2]);
    Some((model_at(neighbour, table, voxel), neighbour.light_at(voxel)))
}

/// The node of a brick that is all one thing, without looking at its voxels: sky, or solid ground with ground all around it.
fn uniform_node(
    input: &BrickNeighbourhood<'_>,
    table: &BlockTable,
    air: u32,
) -> Option<CoarseNode> {
    let center = &input.center;
    let model = uniform_model(center, table)?;
    let light = match center.light {
        Some(light) => light.uniform_value()?.min(255) as u8,
        None => FULL_LIGHT,
    };
    let shape = table.shape(model);
    if shape.is_empty() {
        return Some(CoarseNode::uniform(CoarseCell::empty(air, light)));
    }
    // A whole block that every neighbour hides all of its side of: nothing of it is exposed.
    let buried = table.is_opaque_cube(model)
        && FaceDirection::ALL.iter().all(|&face| {
            input.neighbours[usize::from(face.index())]
                .as_ref()
                .and_then(|neighbour| uniform_model(neighbour, table))
                .is_some_and(|front| table.hides_fully(front, face.opposite()))
        });
    buried.then(|| {
        CoarseNode::uniform(CoarseCell {
            block: center.blocks.uniform_value().expect("the model is uniform"),
            fill: fill_of(shape.filled_cells()),
            light,
            exposure: 0,
        })
    })
}

#[cfg(test)]
mod tests {
    use block_models::{BlockDefinition, Shape};
    use proptest::prelude::*;
    use voxel_grid::ChannelStorage;
    use voxel_space::{BRICK_VOXEL_COUNT, LatticeRotation, LevelOfDetail};

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const GRASS: u32 = 2;
    const SLAB: u32 = 3;
    const LAMP: u32 = 4;
    /// The state of a bottom slab of the `SLAB` block (state 0 is the whole block).
    const BOTTOM: u32 = 1;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let cube = BlockDefinition::cube([0.5; 3]);
        let slab = BlockDefinition::cube([0.5; 3])
            .with_shapes(vec![Shape::full(), Shape::half(FaceDirection::NegativeY)]);
        BlockTable::from_definitions(&[
            Some(&air),
            Some(&cube),
            Some(&cube),
            Some(&slab),
            Some(&cube),
        ])
    }

    /// The storages of a brick whose voxel at `(x, y, z)` is `of(x, y, z)`: its block and its state, and its light if `lit`.
    struct Parts {
        blocks: ChannelStorage,
        states: ChannelStorage,
        light: Option<ChannelStorage>,
    }

    impl Parts {
        fn of(of: impl Fn(usize, usize, usize) -> (u32, u32)) -> Self {
            let mut blocks = vec![0u32; BRICK_VOXEL_COUNT];
            let mut states = vec![0u32; BRICK_VOXEL_COUNT];
            for z in 0..EDGE {
                for y in 0..EDGE {
                    for x in 0..EDGE {
                        let index = LocalVoxelIndex::new(x as u8, y as u8, z as u8).index();
                        (blocks[index], states[index]) = of(x, y, z);
                    }
                }
            }
            Self {
                blocks: ChannelStorage::from_values(&blocks),
                states: ChannelStorage::from_values(&states),
                light: None,
            }
        }

        fn all(block: u32) -> Self {
            Self::of(|_, _, _| (block, 0))
        }

        fn lit(mut self, of: impl Fn(usize, usize, usize) -> u32) -> Self {
            let mut values = vec![0u32; BRICK_VOXEL_COUNT];
            for z in 0..EDGE {
                for y in 0..EDGE {
                    for x in 0..EDGE {
                        values[LocalVoxelIndex::new(x as u8, y as u8, z as u8).index()] =
                            of(x, y, z);
                    }
                }
            }
            self.light = Some(ChannelStorage::from_values(&values));
            self
        }

        fn channels(&self) -> BrickChannels<'_> {
            BrickChannels {
                blocks: &self.blocks,
                states: Some(&self.states),
                orientations: None,
                light: self.light.as_ref(),
            }
        }
    }

    fn node(center: &Parts, neighbours: [Option<&Parts>; 6]) -> CoarseNode {
        coarse_node_of_brick(
            &BrickNeighbourhood {
                center: center.channels(),
                neighbours: neighbours.map(|neighbour| neighbour.map(Parts::channels)),
            },
            &table(),
            AIR,
        )
    }

    fn cell(node: &CoarseNode, x: u8, y: u8, z: u8) -> CoarseCell {
        node.cell(LocalVoxelIndex::new(x, y, z))
    }

    const ONE: u32 = CoarseCell::EXPOSURE_ONE;

    /// Index of a neighbour in `BrickNeighbourhood::neighbours`.
    fn side(face: FaceDirection) -> usize {
        usize::from(face.index())
    }

    #[test]
    fn a_brick_of_air_is_one_empty_cell_as_bright_as_the_brick_and_full_light_where_it_has_no_light()
     {
        let sky = Parts::all(AIR);
        assert_eq!(
            node(&sky, [None; 6]),
            CoarseNode::uniform(CoarseCell::empty(AIR, FULL_LIGHT))
        );
        let dim = Parts::all(AIR).lit(|_, _, _| 77);
        assert_eq!(
            node(&dim, [None; 6]),
            CoarseNode::uniform(CoarseCell::empty(AIR, 77))
        );
    }

    #[test]
    fn a_brick_of_stone_with_stone_all_round_has_no_surface_and_one_with_air_beside_it_has_a_face_of_surface_on_that_side()
     {
        let stone = Parts::all(STONE);
        let all_round = [Some(&stone); 6];
        assert_eq!(
            node(&stone, all_round),
            CoarseNode::uniform(CoarseCell {
                block: STONE,
                fill: 255,
                light: FULL_LIGHT,
                exposure: 0
            })
        );
        // Air above: the top layer of cells has one face each and the rest of the brick none.
        let sky = Parts::all(AIR);
        let mut with_sky = all_round;
        with_sky[side(FaceDirection::PositiveY)] = Some(&sky);
        let found = node(&stone, with_sky);
        for (x, y, z) in [(0u8, 31u8, 0u8), (5, 31, 7), (31, 31, 31)] {
            assert_eq!(cell(&found, x, y, z).exposure, ONE, "({x}, {y}, {z})");
        }
        for (x, y, z) in [(5u8, 30u8, 7u8), (0, 0, 0), (16, 16, 16)] {
            assert_eq!(cell(&found, x, y, z).exposure, 0, "({x}, {y}, {z})");
        }
        // A brick with nothing known beside it is treated as having air beside it, on every side.
        let alone = node(&stone, [None; 6]);
        assert_eq!(cell(&alone, 16, 16, 16).exposure, 0);
        assert_eq!(cell(&alone, 0, 16, 16).exposure, ONE);
        assert_eq!(cell(&alone, 0, 0, 16).exposure, 2 * ONE);
        assert_eq!(cell(&alone, 0, 0, 0).exposure, 3 * ONE);
    }

    #[test]
    fn a_floor_of_grass_on_stone_has_grass_for_its_surface_and_the_stone_under_it_none() {
        let ground = Parts::of(|_, y, _| match y {
            0..=14 => (STONE, 0),
            15 => (GRASS, 0),
            _ => (AIR, 0),
        });
        let stone = Parts::all(STONE);
        let sky = Parts::all(AIR);
        let found = node(
            &ground,
            [
                Some(&ground),
                Some(&ground),
                Some(&stone),
                Some(&sky),
                Some(&ground),
                Some(&ground),
            ],
        );
        let top = cell(&found, 9, 15, 9);
        assert_eq!((top.block, top.fill, top.exposure), (GRASS, 255, ONE));
        let under = cell(&found, 9, 14, 9);
        assert_eq!((under.block, under.fill, under.exposure), (STONE, 255, 0));
        let above = cell(&found, 9, 16, 9);
        assert_eq!((above.block, above.fill, above.exposure), (AIR, 0, 0));
    }

    #[test]
    fn a_slab_is_half_full_shows_its_top_inside_the_voxel_and_the_half_of_each_side_it_has() {
        let alone = Parts::of(|x, y, z| {
            if (x, y, z) == (4, 4, 4) {
                (SLAB, BOTTOM)
            } else {
                (AIR, 0)
            }
        });
        let found = node(&alone, [None; 6]);
        let slab = cell(&found, 4, 4, 4);
        assert_eq!((slab.block, slab.fill), (SLAB, 128));
        // The top (a whole face, half way up), the bottom (a whole face), and four sides of half a face.
        assert_eq!(slab.exposure, ONE + ONE + 4 * (ONE / 2));
        // A slab with another voxel of stone on its side is not exposed there: the half of that side is hidden.
        let beside = Parts::of(|x, y, z| match (x, y, z) {
            (4, 4, 4) => (SLAB, BOTTOM),
            (5, 4, 4) => (STONE, 0),
            _ => (AIR, 0),
        });
        let found = node(&beside, [None; 6]);
        assert_eq!(cell(&found, 4, 4, 4).exposure, ONE + ONE + 3 * (ONE / 2));
    }

    #[test]
    fn a_slab_turned_over_is_the_same_amount_of_slab() {
        let over = LatticeRotation::all()
            .into_iter()
            .position(|rotation| {
                rotation.apply_face(FaceDirection::NegativeY) == FaceDirection::PositiveY
            })
            .unwrap() as u32;
        let alone = Parts::of(|x, y, z| {
            if (x, y, z) == (4, 4, 4) {
                (SLAB, BOTTOM)
            } else {
                (AIR, 0)
            }
        });
        let mut orientations = vec![0u32; BRICK_VOXEL_COUNT];
        orientations[LocalVoxelIndex::new(4, 4, 4).index()] = over;
        let orientations = ChannelStorage::from_values(&orientations);
        let channels = BrickChannels {
            orientations: Some(&orientations),
            ..alone.channels()
        };
        let found = coarse_node_of_brick(
            &BrickNeighbourhood {
                center: channels,
                neighbours: [None; 6],
            },
            &table(),
            AIR,
        );
        let slab = cell(&found, 4, 4, 4);
        assert_eq!((slab.fill, slab.exposure), (128, 2 * ONE + 4 * (ONE / 2)));
    }

    #[test]
    fn a_surface_voxel_is_as_bright_as_the_air_it_looks_out_at_and_a_buried_one_as_its_own() {
        // Stone below y = 16 with a lamp's light in the air above: the air at (5, 16, 5) is bright and the rest of the air dim; the stone is dark.
        let ground =
            Parts::of(|_, y, _| if y < 16 { (STONE, 0) } else { (AIR, 0) }).lit(|x, y, z| {
                match (x, y, z) {
                    (5, 16, 5) => 200,
                    (_, 16.., _) => 30,
                    _ => 5,
                }
            });
        let stone = Parts::all(STONE);
        let found = node(
            &ground,
            [
                Some(&ground),
                Some(&ground),
                Some(&stone),
                Some(&ground),
                Some(&ground),
                Some(&ground),
            ],
        );
        // Under the bright air: bright. Beside it, the stone looks out only up at dim air. Deep in the stone: its own light.
        assert_eq!(cell(&found, 5, 15, 5).light, 200);
        assert_eq!(cell(&found, 6, 15, 5).light, 30);
        assert_eq!(cell(&found, 5, 8, 5).light, 5);
        // And the air keeps its own.
        assert_eq!(cell(&found, 5, 16, 5).light, 200);
        assert_eq!(cell(&found, 9, 20, 9).light, 30);
    }

    #[test]
    fn the_lamp_is_a_block_like_any_other_and_a_solid_voxel_inside_a_brick_with_no_light_channel_is_full_light()
     {
        let lamp = Parts::of(|x, y, z| {
            if (x, y, z) == (3, 3, 3) {
                (LAMP, 0)
            } else {
                (AIR, 0)
            }
        });
        let found = node(&lamp, [None; 6]);
        let cell = cell(&found, 3, 3, 3);
        assert_eq!((cell.block, cell.fill, cell.exposure), (LAMP, 255, 6 * ONE));
        assert_eq!(cell.light, FULL_LIGHT);
    }

    fn random_voxel() -> impl Strategy<Value = (u32, u32)> {
        prop_oneof![
            4 => Just((AIR, 0)),
            3 => Just((STONE, 0)),
            2 => Just((GRASS, 0)),
            1 => Just((SLAB, 0)),
            1 => Just((SLAB, BOTTOM)),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]

        #[test]
        fn what_a_brick_shows_is_kept_by_the_cells_that_are_made_of_it_to_within_their_rounding(
            seed in any::<u64>(),
            choices in proptest::collection::vec(random_voxel(), 64),
        ) {
            // A brick of random voxels (from a small stock, laid out by the seed), and the parent cells made of eight of them: the surface and the fill
            // that the eight showed are the ones the parent shows, to within the rounding of one cell.
            let parts = Parts::of(|x, y, z| {
                let hash = (x as u64 * 73_856_093) ^ (y as u64 * 19_349_663) ^ (z as u64 * 83_492_791) ^ seed;
                choices[(hash.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize % choices.len()]
            });
            let children = node(&parts, [None; 6]);
            let sky = CoarseNode::uniform(CoarseCell::empty(AIR, FULL_LIGHT));
            let parent = CoarseNode::from_children(
                [&children, &sky, &sky, &sky, &sky, &sky, &sky, &sky],
                LevelOfDetail::new(1).unwrap(),
                AIR,
            );
            let child_exposure: u64 = children.cells().map(|cell| u64::from(cell.exposure)).sum();
            let child_fill: u64 = children.cells().map(|cell| u64::from(cell.fill)).sum();
            // Only the first octant of the parent (cells with x, y and z below 16) holds the brick; the rest is sky and has nothing in it.
            let mut parent_exposure = 0u64;
            let mut parent_fill = 0u64;
            let mut occupied = 0u64;
            for cell in parent.cells() {
                parent_exposure += u64::from(cell.exposure);
                parent_fill += u64::from(cell.fill);
                occupied += u64::from(cell.fill > 0);
            }
            // Each parent cell's exposure is rounded to the nearest quarter of a sum: the error of a cell is at most 2 of the sum of four.
            prop_assert!((parent_exposure * 4).abs_diff(child_exposure) <= 2 * occupied, "{parent_exposure} {child_exposure}");
            // Each parent cell's fill is rounded up: by less than 8 of the sum of eight.
            prop_assert!(parent_fill * 8 >= child_fill && parent_fill * 8 < child_fill + 8 * occupied, "{parent_fill} {child_fill}");
        }
    }
}
