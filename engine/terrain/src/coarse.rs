//! The coarse cells of terrain: what a density field looks like from far away (`docs/lod-spec.md`, 38e).
//!
//! A node of detail level `n` is 32 x 32 x 32 cells of `2^n` voxels each. The field is asked about each cell at footprint `n` (so the features finer than a
//! cell are not in it, as for all coarse sampling): whether the cell is all air or all ground from the bounds over it, and for a cell that is neither
//! how full it is from the field at the middle of its column. The block of a cell and the surface it shows are worked out from the fill of the cells
//! around it, with the layering of [`crate::FieldTerrainGenerator`] (grass under open air, then dirt, then stone) at the resolution of a cell.
//!
//! **This is an approximation.** The cell is not the summary of the fine voxels under it, which would take generating all of them; how far it is from
//! that is measured in `world_view/tests/coarse_agreement.rs`.

use std::cell::RefCell;
use std::collections::HashMap;

use coarse_cells::{CELLS_PER_NODE, CoarseCell, CoarseNode, DRAWN_FILL, NODE_EDGE};
use region_octree::BrickCube;
use voxel_space::{LevelOfDetail, VoxelBox, VoxelCoordinate};

use crate::density::DensityField;
use crate::generator::{SURFACE_LAYER_DEPTH, TerrainMaterials};

/// Points along an edge of a cell that the field is asked at when the bounds cannot say whether the cell is air or ground.
const SAMPLES_PER_EDGE: i64 = 4;

/// The coarse node of a cube of bricks of a field, or `None` for detail level 0 (a node of level 0 is a brick, and a brick is generated).
pub(crate) fn coarse_node_of_field(
    field: &dyn DensityField,
    materials: TerrainMaterials,
    cube: BrickCube,
    unlit: u8,
) -> Option<CoarseNode> {
    let level = cube.level();
    if level == 0 {
        return None;
    }
    let detail = LevelOfDetail::new(level)?;
    let sampler = Sampler {
        field,
        detail,
        edge: 1i64 << level,
        origin: cube.origin().origin(),
        halo: RefCell::new(HashMap::new()),
    };
    let air = materials.air;

    // The fill of every cell of the node, found a block at a time where the bounds say the whole block is air or ground.
    let mut fills = vec![0u8; CELLS_PER_NODE];
    sampler.fill_block([0, 0, 0], NODE_EDGE as i64, &mut fills);
    if fills.iter().all(|&fill| fill == 0) {
        return Some(CoarseNode::uniform(CoarseCell::empty(air, unlit)));
    }
    let index = |cell: [i64; 3]| (cell[0] + 32 * cell[1] + 1024 * cell[2]) as usize;
    // What a cell is, inside the node or beside it.
    let fill_at = |cell: [i64; 3]| {
        if cell
            .iter()
            .all(|&component| (0..NODE_EDGE as i64).contains(&component))
        {
            fills[index(cell)]
        } else {
            sampler.fill_outside(cell)
        }
    };

    let mut cells = Vec::with_capacity(CELLS_PER_NODE);
    for z in 0..NODE_EDGE as i64 {
        for y in 0..NODE_EDGE as i64 {
            for x in 0..NODE_EDGE as i64 {
                let cell = [x, y, z];
                let fill = fills[index(cell)];
                if fill == 0 {
                    cells.push(CoarseCell::empty(air, unlit));
                    continue;
                }
                // The surface: a cell with air in it as well as ground has the ground's surface inside it (one face, as for flat ground), and a face
                // for each side that has air (a cell that is not drawn) against it is surface too; the most of the two.
                let mixed = fill < 254;
                let exposure = {
                    let open_sides = [
                        [-1, 0, 0],
                        [1, 0, 0],
                        [0, -1, 0],
                        [0, 1, 0],
                        [0, 0, -1],
                        [0, 0, 1],
                    ]
                    .iter()
                    .filter(|step| fill_at([x + step[0], y + step[1], z + step[2]]) < DRAWN_FILL)
                    .count() as u32;
                    let inside = if mixed { 1 } else { 0 };
                    open_sides.max(inside) * CoarseCell::EXPOSURE_ONE
                };
                // The layers: grass if there is open air over the cell, dirt if the ground over it is less than the depth of the surface layer,
                // else stone. (The ground over it is counted in voxels, from the fill of the cells above.)
                let mut covered = 0u64;
                let mut above = [x, y + 1, z];
                while covered < SURFACE_LAYER_DEPTH as u64 * 255 {
                    let over = fill_at(above);
                    if over < DRAWN_FILL {
                        break;
                    }
                    covered += sampler.edge as u64 * u64::from(over);
                    above[1] += 1;
                }
                // A cell with air in it that has air over it, or a cell the ground goes through, is the top of the ground: grass.
                let block = if covered == 0 || (mixed && fill_at([x, y + 1, z]) < 254) {
                    materials.grass
                } else if covered < SURFACE_LAYER_DEPTH as u64 * 255 {
                    materials.dirt
                } else {
                    materials.stone
                };
                cells.push(CoarseCell {
                    block,
                    fill,
                    light: unlit,
                    exposure,
                });
            }
        }
    }
    Some(CoarseNode::from_cells(cells))
}

struct Sampler<'a> {
    field: &'a dyn DensityField,
    detail: LevelOfDetail,
    /// Voxels along an edge of a cell.
    edge: i64,
    /// The lowest corner of the node.
    origin: VoxelCoordinate,
    /// Cells beside the node that were asked about.
    halo: RefCell<HashMap<[i64; 3], u8>>,
}

impl Sampler<'_> {
    /// The box of `size` cells along an edge whose lowest cell is `cell` (numbered from the node's), if the world has the room for it.
    fn region(&self, cell: [i64; 3], size: i64) -> Option<VoxelBox> {
        let low = self.origin.checked_offset(
            cell[0] * self.edge,
            cell[1] * self.edge,
            cell[2] * self.edge,
        )?;
        let high = low.checked_offset(
            size * self.edge - 1,
            size * self.edge - 1,
            size * self.edge - 1,
        )?;
        VoxelBox::new(low, high)
    }

    /// Fills `out` (the fills of the node, x fastest) for the block of `size` cells along an edge from `base`: all of it at once where the bounds say
    /// the whole block is air or ground, and a cell at a time otherwise.
    fn fill_block(&self, base: [i64; 3], size: i64, out: &mut [u8]) {
        let set = |value: u8, out: &mut [u8]| {
            for z in base[2]..base[2] + size {
                for y in base[1]..base[1] + size {
                    let row = (base[0] + 32 * y + 1024 * z) as usize;
                    out[row..row + size as usize].fill(value);
                }
            }
        };
        if size == 1 {
            let value = self.fill_of_cell(base);
            set(value, out);
            return;
        }
        let Some(region) = self.region(base, size) else {
            set(0, out);
            return;
        };
        let bounds = self.field.bounds(&region, self.detail);
        if bounds.upper() <= 0.0 {
            set(0, out);
        } else if bounds.lower() > 0.0 {
            set(255, out);
        } else {
            let half = size / 2;
            for z in 0..2 {
                for y in 0..2 {
                    for x in 0..2 {
                        self.fill_block(
                            [base[0] + x * half, base[1] + y * half, base[2] + z * half],
                            half,
                            out,
                        );
                    }
                }
            }
        }
    }

    /// The fill of a cell beside the node, remembered.
    fn fill_outside(&self, cell: [i64; 3]) -> u8 {
        if let Some(&fill) = self.halo.borrow().get(&cell) {
            return fill;
        }
        let fill = self.fill_of_cell(cell);
        self.halo.borrow_mut().insert(cell, fill);
        fill
    }

    /// How full one cell is, 0 to 255: 0 if the bounds say it is all air and 255 if they say it is all ground; for a cell that is neither, the share of a
    /// lattice of points in it where the field says ground (1 to 254 if some are and some are not).
    fn fill_of_cell(&self, cell: [i64; 3]) -> u8 {
        let Some(region) = self.region(cell, 1) else {
            return 0;
        };
        let bounds = self.field.bounds(&region, self.detail);
        if bounds.upper() <= 0.0 {
            return 0;
        }
        if bounds.lower() > 0.0 {
            return 255;
        }
        // The bounds could not say: ask the field at a lattice of points in the cell, SAMPLES_PER_EDGE along each edge, each in the voxel that holds the
        // middle of its share of the cell. The share of them that are ground is the fill (so a surface that crosses the cell at a slant is a fill
        // between the extremes, which the middle of the cell alone would not give).
        let mut ground = 0u32;
        for z in 0..SAMPLES_PER_EDGE {
            for y in 0..SAMPLES_PER_EDGE {
                for x in 0..SAMPLES_PER_EDGE {
                    let at = |sample: i64, origin: i64| {
                        origin + (2 * sample + 1) * self.edge / (2 * SAMPLES_PER_EDGE)
                    };
                    let position = VoxelCoordinate::try_new(
                        at(x, region.min().x()),
                        at(y, region.min().y()),
                        at(z, region.min().z()),
                    );
                    if position.is_ok_and(|position| self.field.sample(position, self.detail) > 0.0)
                    {
                        ground += 1;
                    }
                }
            }
        }
        let total = (SAMPLES_PER_EDGE * SAMPLES_PER_EDGE * SAMPLES_PER_EDGE) as u32;
        match ground {
            0 => 0,
            count if count == total => 255,
            count => (((count * 255 + total / 2) / total) as u8).clamp(1, 254),
        }
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::{BrickCoordinate, VoxelBox};

    use super::*;
    use crate::interval::Interval;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const DIRT: u32 = 2;
    const GRASS: u32 = 3;
    const UNLIT: u8 = 9;

    fn materials() -> TerrainMaterials {
        TerrainMaterials {
            air: AIR,
            stone: STONE,
            dirt: DIRT,
            grass: GRASS,
        }
    }

    /// Flat ground: solid below `height` (a voxel is ground when its middle is below it). The field changes by 1 for each voxel of height.
    #[derive(Debug)]
    struct Flat(f64);

    impl DensityField for Flat {
        fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
            self.0 - (position.y() as f64 + 0.5)
        }

        fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
            Interval::new(
                self.0 - (region.max().y() as f64 + 0.5),
                self.0 - (region.min().y() as f64 + 0.5),
            )
        }
    }

    fn node(field: &dyn DensityField, brick: [i64; 3], level: u8) -> Option<CoarseNode> {
        let cube =
            BrickCube::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), level).unwrap();
        coarse_node_of_field(field, materials(), cube, UNLIT)
    }

    fn cell(node: &CoarseNode, x: u8, y: u8, z: u8) -> CoarseCell {
        node.cell(voxel_space::LocalVoxelIndex::new(x, y, z))
    }

    #[test]
    fn a_brick_is_generated_and_not_summarised_so_detail_level_0_has_no_coarse_node() {
        assert_eq!(node(&Flat(50.0), [0, 0, 0], 0), None);
    }

    #[test]
    fn a_node_wholly_in_the_air_is_one_empty_cell_with_the_light_given() {
        // Level 2: cells of 4 voxels, the node 128 voxels, from y = 128 up (brick y = 4).
        let sky = node(&Flat(50.0), [0, 4, 0], 2).unwrap();
        assert_eq!(sky, CoarseNode::uniform(CoarseCell::empty(AIR, UNLIT)));
    }

    #[test]
    fn a_node_wholly_in_the_ground_is_stone_all_through_and_its_edges_show_surface_only_where_air_is_beside_it()
     {
        // Level 2 node from y = -128 to 0, under ground at 50: deep ground, and nothing but ground above it within the node's own.
        let deep = node(&Flat(50.0), [0, -4, 0], 2).unwrap();
        assert!(
            deep.cells()
                .all(|cell| cell.block == STONE && cell.fill == 255)
        );
        assert_eq!(cell(&deep, 5, 5, 5).exposure, 0);
        // Ground reaching up to 50, over the node's top at 0: the ground is above it too, so it has no surface.
        assert_eq!(cell(&deep, 5, 31, 5).exposure, 0);
    }

    #[test]
    fn flat_ground_is_grass_on_the_cells_the_surface_goes_through_dirt_under_them_to_the_depth_of_the_surface_layer_and_stone_below()
     {
        // Level 1: cells of 2 voxels. Ground up to 65: voxel 64 is the top voxel (its middle, 64.5, is under 65). Brick y = 2 holds voxels 64 to 95;
        // cell y = 0 is voxels 64 and 65: half ground.
        let found = node(&Flat(65.0), [0, 2, 0], 1).unwrap();
        // The node is 64 voxels from y = 64 up (level 1, brick 0..2): cells of 2 voxels, the lowest holding voxels 64 and 65.
        let surface = cell(&found, 8, 0, 8);
        assert_eq!(surface.block, GRASS);
        assert!((100..=160).contains(&surface.fill), "fill {}", surface.fill);
        assert_eq!(surface.exposure, CoarseCell::EXPOSURE_ONE);
        assert_eq!(cell(&found, 8, 1, 8), CoarseCell::empty(AIR, UNLIT));
    }

    #[test]
    fn the_layers_under_the_surface_go_grass_dirt_stone_at_the_resolution_of_the_cells() {
        // Level 1, the node from y = 64 (brick 2): cells of 2 voxels, cell 0 is voxels 64 and 65. Ground to 71: the top ground voxel is 70, in cell 3
        // (voxels 70 and 71), which is half ground.
        let found = node(&Flat(71.0), [0, 2, 0], 1).unwrap();
        assert_eq!(cell(&found, 3, 3, 3).block, GRASS);
        assert_eq!(cell(&found, 3, 4, 3).fill, 0);
        // Under it the ground over a cell is counted: over cell 2 (voxels 68, 69) one voxel of cell 3; over cell 1 (66, 67) two cells and a half
        // more, three voxels in all: still less than the surface layer (4), so dirt. Over cell 0 (64, 65) there are five voxels over it: stone.
        assert_eq!(cell(&found, 3, 2, 3).block, DIRT);
        assert_eq!(cell(&found, 3, 1, 3).block, DIRT);
        assert_eq!(cell(&found, 3, 0, 3).block, STONE);
    }

    #[test]
    fn a_cell_that_has_ground_in_the_first_voxel_of_its_column_and_none_after_is_a_quarter_full_at_level_2()
     {
        // Level 2: cells of 4 voxels. Ground to 65: in the cell of voxels 64 to 67 the first voxel is ground: a quarter.
        let found = node(&Flat(65.0), [0, 0, 0], 2).unwrap();
        // The node from y = 0: cell 16 is voxels 64 to 67.
        let surface = cell(&found, 4, 16, 4);
        assert!((60..=68).contains(&surface.fill), "fill {}", surface.fill);
        assert_eq!(surface.block, GRASS);
    }

    #[test]
    fn the_surface_of_a_slope_and_a_cliff_shows_the_faces_of_the_cells_that_have_air_beside_them() {
        // A step: ground at 100 for x below 0 and at 40 for x from 0 (the field is the larger of the two cut at x = 0; its bounds are wide but sound).
        #[derive(Debug)]
        struct Step;
        impl DensityField for Step {
            fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
                let height = if position.x() < 0 { 100.0 } else { 40.0 };
                height - (position.y() as f64 + 0.5)
            }
            fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
                let (low, high): (f64, f64) = (
                    if region.min().x() < 0 { 100.0 } else { 40.0 },
                    if region.max().x() < 0 { 100.0 } else { 40.0 },
                );
                let top = low.max(high);
                let bottom = low.min(high);
                Interval::new(
                    bottom - (region.max().y() as f64 + 0.5),
                    top - (region.min().y() as f64 + 0.5),
                )
            }
        }
        // Level 1, the node of bricks -2 and -1 along x (voxels -64 to -1, cells of 2) and from y = 0: all of it ground to 100, and the column of cells
        // beside it at x = 32 (voxels 0 and 1) ground to 40. The cell at x = 31, y = 25 (voxels 50 and 51) has air beside it.
        let found = node(&Step, [-2, 0, 0], 1).unwrap();
        let wall = cell(&found, 31, 25, 8);
        assert_eq!(wall.fill, 255);
        // One side of it is air (the cell beside it at x = 16): a face of surface, and the ground above and below it is ground.
        assert_eq!(wall.exposure, CoarseCell::EXPOSURE_ONE);
        // Beside the air, in the open, it is not grass (the ground is over it): dirt or stone.
        assert_ne!(wall.block, GRASS);
    }

    /// Ground everywhere but for a ball of air of this radius (in voxels) round a point: the field is the distance from the point, less the radius.
    #[derive(Debug)]
    struct Bubble {
        centre: [f64; 3],
        radius: f64,
    }

    impl DensityField for Bubble {
        fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
            let at = [position.x(), position.y(), position.z()];
            let distance_squared: f64 = (0..3)
                .map(|axis| {
                    let offset = at[axis] as f64 + 0.5 - self.centre[axis];
                    offset * offset
                })
                .sum();
            distance_squared.sqrt() - self.radius
        }

        fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
            let (low, high) = (region.min(), region.max());
            let (mut nearest_squared, mut farthest_squared) = (0.0f64, 0.0f64);
            for axis in 0..3 {
                let (a, b) = (
                    [low.x(), low.y(), low.z()][axis] as f64 + 0.5 - self.centre[axis],
                    [high.x(), high.y(), high.z()][axis] as f64 + 0.5 - self.centre[axis],
                );
                let near = if a <= 0.0 && b >= 0.0 {
                    0.0
                } else {
                    a.abs().min(b.abs())
                };
                nearest_squared += near * near;
                let far = a.abs().max(b.abs());
                farthest_squared += far * far;
            }
            Interval::new(
                nearest_squared.sqrt() - self.radius,
                farthest_squared.sqrt() - self.radius,
            )
        }
    }

    #[test]
    fn a_cell_with_a_pocket_of_air_in_it_has_a_surface_of_its_own_though_every_cell_beside_it_is_ground()
     {
        // A ball of air a voxel and a half in radius in the middle of the cell at (8, 8, 8) of a node of level 2 (cells of 4 voxels, from voxel 32).
        let bubble = Bubble {
            centre: [
                32.0 * 4.0 + 8.0 * 4.0 + 2.0,
                32.0 * 4.0 + 8.0 * 4.0 + 2.0,
                32.0 * 4.0 + 8.0 * 4.0 + 2.0,
            ],
            radius: 1.5,
        };
        let found = node(&bubble, [4, 4, 4], 2).unwrap();
        let pocket = cell(&found, 8, 8, 8);
        assert!((100..254).contains(&pocket.fill), "fill {}", pocket.fill);
        assert_eq!(pocket.exposure, CoarseCell::EXPOSURE_ONE);
        // The ground round it is ground all through, with no surface.
        assert_eq!(cell(&found, 8, 9, 8).exposure, 0);
        assert_eq!(cell(&found, 7, 8, 8).fill, 255);
    }

    /// Ground below a steep slope: the field falls by 1 for each voxel up and 4 for each voxel along x.
    #[derive(Debug)]
    struct Steep;

    impl DensityField for Steep {
        fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
            200.0 - (position.y() as f64 + 0.5) - 4.0 * (position.x() as f64 + 0.5)
        }

        fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
            Interval::new(
                200.0 - (region.max().y() as f64 + 0.5) - 4.0 * (region.max().x() as f64 + 0.5),
                200.0 - (region.min().y() as f64 + 0.5) - 4.0 * (region.min().x() as f64 + 0.5),
            )
        }
    }

    #[test]
    fn where_the_surface_goes_through_a_cell_and_the_cell_over_it_the_lower_is_still_the_top_of_the_ground_and_is_grass()
     {
        let found = node(&Steep, [0, 0, 0], 2).unwrap();
        let mut seen = 0;
        for x in 0..32u8 {
            for y in 0..31u8 {
                let (below, above) = (cell(&found, x, y, 4), cell(&found, x, y + 1, 4));
                if (128..254).contains(&below.fill) && (128..254).contains(&above.fill) {
                    seen += 1;
                    assert_eq!(below.block, GRASS, "({x}, {y})");
                }
            }
        }
        assert!(
            seen > 0,
            "the slope never went through two cells over each other"
        );
    }

    /// A digest of the cells of a node (FNV-1a over each cell's block, fill, light and exposure, in order).
    fn digest(node: &CoarseNode) -> u64 {
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        for cell in node.cells() {
            for value in [
                cell.block,
                u32::from(cell.fill),
                u32::from(cell.light),
                cell.exposure,
            ] {
                for byte in value.to_le_bytes() {
                    state ^= u64::from(byte);
                    state = state.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        state
    }

    #[test]
    fn the_coarse_nodes_of_rolling_hills_are_pinned() {
        // Like the bricks of a generator, its coarse nodes are not to change once there is a recording of them (a replay must draw the same
        // picture). These two are the fingerprint: if this fails, add a new generator version instead of changing the output. Windows and Linux agree.
        let fixture = voxel_grid::test_support::StandardTestLayout::default();
        let generator = crate::generator::rolling_hills(
            crate::generator::RollingHillsSettings::for_one_metre_voxels(2024),
            fixture.material,
            materials(),
        );
        let digests: Vec<u64> = [([8, 0, -8], 3u8), ([64, 0, 64], 6)]
            .into_iter()
            .map(|(origin, level)| {
                let cube =
                    BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), level)
                        .unwrap();
                let node = crate::TerrainGenerator::coarse_node(&generator, cube, UNLIT).unwrap();
                digest(&node)
            })
            .collect();
        assert_eq!(digests, PINNED_DIGESTS);
    }

    const PINNED_DIGESTS: [u64; 2] = [12_276_066_251_273_190_578, 727_442_832_022_872_305];

    #[test]
    fn the_same_cube_gives_the_same_node_every_time() {
        let first = node(&Flat(65.0), [8, 8, -8], 3).unwrap();
        let second = node(&Flat(65.0), [8, 8, -8], 3).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_node_of_the_coarsest_detail_there_is_works_and_is_ground_with_grass_on_the_cells_next_to_the_air_above()
     {
        // Detail level 40: cells of 2^40 voxels, a node of 2^45, which must be aligned to 2^40 bricks. This one is the one under the ground, up to y = 0, with
        // ground to 50 above its top: the ground all through, and the layer of cells at the top is under cells that are almost all air (their ground is 50
        // voxels of a million million), so it is the top of the ground there.
        let cube = BrickCube::new(BrickCoordinate::new(0, -(1 << 40), 0), 40).unwrap();
        let found = coarse_node_of_field(&Flat(50.0), materials(), cube, UNLIT).unwrap();
        let cells = |height: u8| cell(&found, 3, height, 3);
        assert_eq!(cells(0).block, STONE);
        assert_eq!(cells(0).exposure, 0);
        assert_eq!(cells(31).fill, 255);
        assert_eq!(
            (cells(31).block, cells(31).exposure),
            (GRASS, CoarseCell::EXPOSURE_ONE)
        );
    }
}
