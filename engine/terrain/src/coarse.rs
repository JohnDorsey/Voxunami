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
use voxel_space::{BRICK_EDGE_LENGTH, LevelOfDetail, VoxelBox, VoxelCoordinate};

use crate::density::DensityField;
use crate::generator::{SURFACE_LAYER_DEPTH, TerrainMaterials};

/// Points along an edge of a cell that the field is asked at when the bounds cannot say whether the cell is air or ground.
const SAMPLES_PER_EDGE: i64 = 4;

/// Whether a cube of bricks of a field could show anything (see `TerrainGenerator::may_show`): not if the field is air through the cube at its level of detail and
/// at every finer one, nor if it is ground through the cube and the cube's neighbours, which are as many cubes all round it.
pub(crate) fn cube_may_show(field: &dyn DensityField, cube: BrickCube) -> bool {
    let Some(detail) = LevelOfDetail::new(cube.level()) else {
        return true;
    };
    let edge = (cube.edge_length() * BRICK_EDGE_LENGTH as u64) as i64;
    let origin = cube.origin().origin();
    let Some(region) = origin
        .checked_offset(edge - 1, edge - 1, edge - 1)
        .and_then(|high| VoxelBox::new(origin, high))
    else {
        return true;
    };
    if field.bounds_through_finer(&region, detail).upper() <= 0.0 {
        return false;
    }
    let Some(around) = origin
        .checked_offset(-edge, -edge, -edge)
        .zip(origin.checked_offset(2 * edge - 1, 2 * edge - 1, 2 * edge - 1))
        .and_then(|(low, high)| VoxelBox::new(low, high))
    else {
        return true;
    };
    field.bounds_through_finer(&around, detail).lower() <= 0.0
}

/// The coarse node of a cube of bricks of a field, or `None` for detail level 0 (a node of level 0 is a brick, and a brick is generated).
pub(crate) fn coarse_node_of_field(
    field: &dyn DensityField,
    materials: TerrainMaterials,
    cube: BrickCube,
    unlit: u8,
) -> Option<CoarseNode> {
    let sampler = sampler_of(field, cube)?;
    // Ground that the bounds say has ground all round it and over it is the same everywhere, without asking about each of 32,768 cells: the half of the
    // world that is under the other half.
    if sampler.is_sealed_ground() {
        return Some(CoarseNode::uniform(CoarseCell {
            block: materials.stone,
            fill: 255,
            light: unlit,
            exposure: 0,
        }));
    }
    Some(coarse_node_by_cells(&sampler, materials, unlit))
}

/// The same as [`coarse_node_of_field`], asking about every cell (what it was before the shortcut for sealed ground; the tests check them against each other).
#[cfg(test)]
pub(crate) fn coarse_node_of_field_cell_by_cell(
    field: &dyn DensityField,
    materials: TerrainMaterials,
    cube: BrickCube,
    unlit: u8,
) -> Option<CoarseNode> {
    let sampler = sampler_of(field, cube)?;
    Some(coarse_node_by_cells(&sampler, materials, unlit))
}

/// The fill of every cell of the node of `cube`, found by asking the field about each cell (what coarse nodes are made from; for tests that compare another
/// way of finding the fills with it).
#[cfg(test)]
pub(crate) fn fills_of_field_cell_by_cell(
    field: &dyn DensityField,
    cube: BrickCube,
) -> Option<Vec<u8>> {
    let sampler = sampler_of(field, cube)?;
    let mut fills = vec![0u8; CELLS_PER_NODE];
    sampler.fill_block([0, 0, 0], NODE_EDGE as i64, &mut fills);
    Some(fills)
}

fn sampler_of(field: &dyn DensityField, cube: BrickCube) -> Option<Sampler<'_>> {
    let level = cube.level();
    if level == 0 {
        return None;
    }
    Some(Sampler {
        field,
        detail: LevelOfDetail::new(level)?,
        edge: 1i64 << level,
        origin: cube.origin().origin(),
        halo: RefCell::new(HashMap::new()),
    })
}

fn coarse_node_by_cells(
    sampler: &Sampler<'_>,
    materials: TerrainMaterials,
    unlit: u8,
) -> CoarseNode {
    let air = materials.air;

    // The fill of every cell of the node, found a block at a time where the bounds say the whole block is air or ground.
    let mut fills = vec![0u8; CELLS_PER_NODE];
    sampler.fill_block([0, 0, 0], NODE_EDGE as i64, &mut fills);
    if fills.iter().all(|&fill| fill == 0) {
        return CoarseNode::uniform(CoarseCell::empty(air, unlit));
    }
    coarse_node_from_fills(
        &fills,
        &|cell| sampler.fill_outside(cell),
        [0, 1, 0],
        sampler.edge,
        materials,
        unlit,
    )
}

/// The cells of a node from how full each of its cells is (`fills`, x fastest, 0 to 255) and how full the cells beside it are (`fill_outside`, by their
/// numbers counted from the node's), for cells `edge` voxels across: the block of a cell (grass with open air over it, dirt with less than the depth of the
/// surface layer of ground over it, stone otherwise) and the surface it shows. `up` is the step to the cell over a cell: the layers look along it, outward.
pub(crate) fn coarse_node_from_fills(
    fills: &[u8],
    fill_outside: &dyn Fn([i64; 3]) -> u8,
    up: [i64; 3],
    edge: i64,
    materials: TerrainMaterials,
    unlit: u8,
) -> CoarseNode {
    let air = materials.air;
    let index = |cell: [i64; 3]| (cell[0] + 32 * cell[1] + 1024 * cell[2]) as usize;
    // What a cell is, inside the node or beside it.
    let fill_at = |cell: [i64; 3]| {
        if cell
            .iter()
            .all(|&component| (0..NODE_EDGE as i64).contains(&component))
        {
            fills[index(cell)]
        } else {
            fill_outside(cell)
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
                let over_the_cell = [x + up[0], y + up[1], z + up[2]];
                let mut above = over_the_cell;
                while covered < SURFACE_LAYER_DEPTH as u64 * 255 {
                    let over = fill_at(above);
                    if over < DRAWN_FILL {
                        break;
                    }
                    covered += edge as u64 * u64::from(over);
                    above = [above[0] + up[0], above[1] + up[1], above[2] + up[2]];
                }
                // A cell with air in it that has air over it, or a cell the ground goes through, is the top of the ground: grass.
                let block = if covered == 0 || (mixed && fill_at(over_the_cell) < 254) {
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
    CoarseNode::from_cells(cells)
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

    /// Whether the bounds say that all of the node, the cells round it, and the cells over it that the layers of the surface look at, are ground: then every
    /// cell of the node is stone, whole and with no surface.
    fn is_sealed_ground(&self) -> bool {
        // The layers look up until the ground over a cell is as deep as the surface layer.
        let above = (SURFACE_LAYER_DEPTH as i64 + self.edge - 1) / self.edge;
        let low = self
            .origin
            .checked_offset(-self.edge, -self.edge, -self.edge);
        let high = self.origin.checked_offset(
            (NODE_EDGE as i64 + 1) * self.edge - 1,
            (NODE_EDGE as i64 + above) * self.edge - 1,
            (NODE_EDGE as i64 + 1) * self.edge - 1,
        );
        let Some(region) = low
            .zip(high)
            .and_then(|(low, high)| VoxelBox::new(low, high))
        else {
            return false;
        };
        self.field.bounds(&region, self.detail).lower() > 0.0
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

/// A digest of the cells of a node (FNV-1a over each cell's block, fill, light and exposure, in order), for tests that pin the nodes of a generator.
#[cfg(test)]
pub(crate) fn node_digest(node: &CoarseNode) -> u64 {
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

        fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
            self.bounds(region, footprint)
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

    /// Every node of this field that is wholly ground or wholly air or not, at levels 1 to 6 over a stretch of bricks: the shortcut for sealed ground gives
    /// what asking about every cell gives.
    fn assert_the_shortcut_changes_nothing(
        field: &dyn DensityField,
        bricks: std::ops::Range<i64>,
        at_least_solid: usize,
    ) {
        let mut sealed = 0;
        for level in 1..=6u8 {
            let edge = 1i64 << level;
            let aligned = |range: &std::ops::Range<i64>| {
                (range.start.div_euclid(edge)..=(range.end.div_euclid(edge))).map(move |n| n * edge)
            };
            for x in aligned(&bricks) {
                for y in aligned(&bricks) {
                    for z in aligned(&bricks) {
                        let cube = BrickCube::new(BrickCoordinate::new(x, y, z), level).unwrap();
                        let quick = coarse_node_of_field(field, materials(), cube, UNLIT);
                        let slow =
                            coarse_node_of_field_cell_by_cell(field, materials(), cube, UNLIT);
                        assert_eq!(quick, slow, "{cube:?}");
                        sealed += usize::from(
                            quick
                                .as_ref()
                                .and_then(CoarseNode::uniform_cell)
                                .is_some_and(|cell| cell.fill == 255),
                        );
                    }
                }
            }
        }
        assert!(
            sealed >= at_least_solid,
            "only {sealed} nodes were solid to the end"
        );
    }

    #[test]
    fn the_shortcut_for_ground_with_ground_all_round_it_changes_nothing_on_flat_ground_a_step_and_a_bubble()
     {
        assert_the_shortcut_changes_nothing(&Flat(50.0), -6..6, 10);
        assert_the_shortcut_changes_nothing(&Flat(77.3), -6..6, 10);
        // The ground reaching to just over the top of a node: the layers of the surface are in the node.
        for height in [64.5, 66.0, 67.5, 130.0] {
            assert_the_shortcut_changes_nothing(&Flat(height), -6..6, 10);
        }
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
                Interval::new(
                    low.min(high) - (region.max().y() as f64 + 0.5),
                    low.max(high) - (region.min().y() as f64 + 0.5),
                )
            }
        }
        assert_the_shortcut_changes_nothing(&Step, -6..6, 10);
        // A slab of ground in the air: the cells under and over it are air.
        #[derive(Debug)]
        struct Slab {
            low: f64,
            high: f64,
        }
        impl DensityField for Slab {
            fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
                let y = position.y() as f64 + 0.5;
                (y - self.low).min(self.high - y)
            }
            fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
                let (bottom, top) = (region.min().y() as f64 + 0.5, region.max().y() as f64 + 0.5);
                let value = |y: f64| (y - self.low).min(self.high - y);
                let middle = (self.low + self.high) / 2.0;
                let upper = if (bottom..=top).contains(&middle) {
                    (self.high - self.low) / 2.0
                } else {
                    value(bottom).max(value(top))
                };
                Interval::new(value(bottom).min(value(top)), upper)
            }
        }
        assert_the_shortcut_changes_nothing(
            &Slab {
                low: 64.0,
                high: 300.0,
            },
            -6..6,
            0,
        );
        assert_the_shortcut_changes_nothing(
            &Slab {
                low: 63.0,
                high: 129.0,
            },
            -6..6,
            0,
        );
        assert_the_shortcut_changes_nothing(
            &Bubble {
                centre: [1000.0, 1000.0, 1000.0],
                radius: 300.0,
            },
            -6..6,
            10,
        );
    }

    #[test]
    fn a_node_the_bubble_comes_up_to_is_not_taken_for_sealed_ground() {
        // Ground but for a ball of air that reaches into the cell beside the node: the cell that has air beside it has a surface.
        let field = Bubble {
            centre: [-10.0, 16.0, 16.0],
            radius: 10.0,
        };
        let cube = BrickCube::new(BrickCoordinate::new(0, 0, 0), 1).unwrap();
        let found = coarse_node_of_field(&field, materials(), cube, UNLIT).unwrap();
        assert!(found.uniform_cell().is_none(), "the node is not uniform");
        assert_eq!(
            Some(found),
            coarse_node_of_field_cell_by_cell(&field, materials(), cube, UNLIT)
        );
    }

    fn cube_at(origin: [i64; 3], level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), level).unwrap()
    }

    /// Ground between two planes across one axis: a voxel is ground where the middle of it is between `low` and `high` along the axis, and air above and below.
    #[derive(Debug)]
    struct Plate {
        axis: usize,
        low: f64,
        high: f64,
    }

    impl Plate {
        fn at(&self, middle: f64) -> f64 {
            (middle - self.low).min(self.high - middle)
        }
    }

    impl DensityField for Plate {
        fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
            let along = [position.x(), position.y(), position.z()][self.axis];
            self.at(along as f64 + 0.5)
        }

        fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
            let (low, high) = (region.min(), region.max());
            let first = [low.x(), low.y(), low.z()][self.axis] as f64 + 0.5;
            let last = [high.x(), high.y(), high.z()][self.axis] as f64 + 0.5;
            let middle = (self.low + self.high) / 2.0;
            let upper = if (first..=last).contains(&middle) {
                (self.high - self.low) / 2.0
            } else {
                self.at(first).max(self.at(last))
            };
            Interval::new(self.at(first).min(self.at(last)), upper)
        }

        fn bounds_through_finer(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
            self.bounds(region, footprint)
        }
    }

    #[test]
    fn a_cube_is_judged_by_every_voxel_of_it_and_by_all_the_room_round_it_on_every_axis_and_both_sides()
     {
        for axis in 0..3 {
            let along = |count: i64| {
                let mut origin = [0i64; 3];
                origin[axis] = count;
                origin
            };
            let plate = |low, high| Plate { axis, low, high };
            // The only ground is the last row of voxels of the brick at the origin: that is something to show.
            assert!(
                cube_may_show(&plate(31.0, 32.0), cube_at(along(0), 0)),
                "axis {axis}"
            );
            // Ground through the brick above it, and through the ones beyond, with air under it: its underside is seen.
            assert!(
                cube_may_show(&plate(32.0, 200.0), cube_at(along(1), 0)),
                "axis {axis}"
            );
            // The same with the air over it.
            assert!(
                cube_may_show(&plate(-200.0, 64.0), cube_at(along(1), 0)),
                "axis {axis}"
            );
            // Ground all through the room, and not a thing to show.
            assert!(
                !cube_may_show(&plate(-200.0, 200.0), cube_at(along(1), 0)),
                "axis {axis}"
            );
        }
    }

    #[test]
    fn a_cliff_that_faces_either_way_is_seen_from_the_cubes_of_the_high_side_whichever_side_that_is()
     {
        // Ground 300 high on one side of a plane across x or z and 40 high on the other: a cube of the high side from height 0 to 127 is ground all through, with ground
        // above and below it, and has air beside it across the plane (over 40).
        #[derive(Debug)]
        struct Cliff {
            axis: usize,
            high_side_is_positive: bool,
        }
        impl Cliff {
            fn height(&self, along: i64) -> f64 {
                if (along >= 0) == self.high_side_is_positive {
                    300.0
                } else {
                    40.0
                }
            }
        }
        impl DensityField for Cliff {
            fn sample(&self, position: VoxelCoordinate, _: LevelOfDetail) -> f64 {
                let along = [position.x(), position.y(), position.z()][self.axis];
                self.height(along) - (position.y() as f64 + 0.5)
            }
            fn bounds(&self, region: &VoxelBox, _: LevelOfDetail) -> Interval {
                let (low, high) = (region.min(), region.max());
                let first = self.height([low.x(), low.y(), low.z()][self.axis]);
                let last = self.height([high.x(), high.y(), high.z()][self.axis]);
                Interval::new(
                    first.min(last) - (high.y() as f64 + 0.5),
                    first.max(last) - (low.y() as f64 + 0.5),
                )
            }
            fn bounds_through_finer(
                &self,
                region: &VoxelBox,
                footprint: LevelOfDetail,
            ) -> Interval {
                self.bounds(region, footprint)
            }
        }
        for axis in [0usize, 2] {
            for high_side_is_positive in [false, true] {
                let cliff = Cliff {
                    axis,
                    high_side_is_positive,
                };
                // A level 2 cube is 128 voxels: the one from 0 to 127 along the axis is on the positive side, the one from -128 to -1 on the negative.
                let high_side = if high_side_is_positive { 0 } else { -4 };
                let mut origin = [0i64; 3];
                origin[axis] = high_side;
                // Heights 0 to 127 (the cube is ground all through, as are the cubes over and under it), against the plane: across it the ground is only 40 high.
                origin[1] = 0;
                assert!(
                    cube_may_show(&cliff, cube_at(origin, 2)),
                    "axis {axis}, high side positive {high_side_is_positive}"
                );
                // Two cubes further from the plane on the high side, there is ground all round it.
                origin[axis] = if high_side_is_positive { 8 } else { -12 };
                assert!(
                    !cube_may_show(&cliff, cube_at(origin, 2)),
                    "axis {axis}, high side positive {high_side_is_positive}"
                );
            }
        }
    }

    #[test]
    fn a_cube_is_judged_at_its_own_level_of_detail_and_every_finer_one() {
        // A field whose ground is at 20 from detail level 4 up (a coarse octave that is there when the finer ones are dropped), and at 0 below that: a cube of
        // 512 voxels at level 4 has ground in it at its own level, which the finest level does not show.
        #[derive(Debug)]
        struct Raised;
        impl Raised {
            fn height(footprint: LevelOfDetail) -> f64 {
                if footprint.level() >= 4 { 20.0 } else { 0.0 }
            }
        }
        impl DensityField for Raised {
            fn sample(&self, position: VoxelCoordinate, footprint: LevelOfDetail) -> f64 {
                Self::height(footprint) - (position.y() as f64 + 0.5)
            }
            fn bounds(&self, region: &VoxelBox, footprint: LevelOfDetail) -> Interval {
                let height = Self::height(footprint);
                Interval::new(
                    height - (region.max().y() as f64 + 0.5),
                    height - (region.min().y() as f64 + 0.5),
                )
            }
            fn bounds_through_finer(
                &self,
                region: &VoxelBox,
                footprint: LevelOfDetail,
            ) -> Interval {
                (0..=footprint.level())
                    .map(|level| self.bounds(region, LevelOfDetail::new(level).unwrap()))
                    .reduce(Interval::hull)
                    .unwrap()
            }
        }
        assert!(cube_may_show(&Raised, cube_at([0, 0, 0], 4)));
        // At the finest levels it is air, as bounds at level 0 say.
        assert!(!cube_may_show(&Raised, cube_at([0, 0, 0], 3)));
    }

    #[test]
    fn a_cube_of_flat_ground_may_show_something_only_if_the_surface_or_the_air_is_in_it_or_beside_it()
     {
        let field = Flat(100.0);
        // Level 2: cubes of 4 bricks, 128 voxels. The ground reaches 100, so the cube from voxel 0 to 127 holds the surface, the one above it (128 to 255) is all air.
        assert!(cube_may_show(&field, cube_at([0, 0, 0], 2)));
        assert!(!cube_may_show(&field, cube_at([0, 4, 0], 2)), "air");
        assert!(!cube_may_show(&field, cube_at([0, 40, 0], 2)), "air");
        // The cube from -128 to -1 has the surface in the cube above it: ground that has air one cube away is not buried.
        assert!(cube_may_show(&field, cube_at([0, -4, 0], 2)));
        // The one from -256 to -129 has ground all round it (the cube above is -128 to -1, below the surface).
        assert!(!cube_may_show(&field, cube_at([0, -8, 0], 2)), "buried");
        assert!(!cube_may_show(&field, cube_at([-12, -80, 40], 2)), "buried");
        // Sideways it is the same all along.
        assert!(cube_may_show(&field, cube_at([36, 0, -20], 2)));
        // A brick is a cube too.
        assert!(cube_may_show(&field, cube_at([3, 2, 3], 0)));
        assert!(!cube_may_show(&field, cube_at([3, 20, 3], 0)));
        assert!(!cube_may_show(&field, cube_at([3, -20, 3], 0)));
        // At the edges: a voxel is air when the middle of it is at the surface or over it. With the surface at the middle of the lowest voxels of a brick, all of it is air.
        assert!(!cube_may_show(&Flat(32.5), cube_at([0, 1, 0], 0)));
        assert!(cube_may_show(&Flat(33.5), cube_at([0, 1, 0], 0)));
        // And a brick is buried when the whole cube round it is ground: not when the middle of the top voxel of that is at the surface, which is air.
        assert!(cube_may_show(&Flat(-0.5), cube_at([0, -2, 0], 0)));
        assert!(!cube_may_show(&Flat(0.5), cube_at([0, -2, 0], 0)));
        // A cube so far off that the room round it is not in the world may show something.
        assert!(cube_may_show(
            &field,
            BrickCube::containing(BrickCoordinate::new(1 << 54, 0, 0), 40)
        ));
    }

    #[test]
    fn what_a_cube_that_may_show_nothing_holds_is_air_or_stone_all_through_at_every_level() {
        use crate::generator::{RollingHillsSettings, TerrainGenerator, rolling_hills};
        use voxel_grid::test_support::StandardTestLayout;
        let fixture = StandardTestLayout::default();
        let mut pruned = 0;
        let mut kept = 0;
        for caves in [false, true] {
            let settings = RollingHillsSettings {
                cave_half_thickness: if caves { 0.08 } else { 0.0 },
                ..RollingHillsSettings::for_one_metre_voxels(2024)
            };
            let generator = rolling_hills(settings, fixture.material, materials());
            for level in 0..=3u8 {
                let width = 1i64 << level;
                // Cubes from well under the surface (near 64 voxels, hills 100 either way) to well over it.
                for x in 0..3 {
                    for z in 0..3 {
                        for y in -12..14 {
                            let cube = cube_at([x * width * 2, y * width, z * width * 2], level);
                            if generator.may_show(cube) {
                                kept += 1;
                                continue;
                            }
                            pruned += 1;
                            let edge = cube.edge_length() as i64;
                            let origin = cube.origin();
                            for bx in 0..edge {
                                for by in 0..edge {
                                    for bz in 0..edge {
                                        let brick = generator.generate_brick(
                                            &fixture.layout,
                                            BrickCoordinate::new(
                                                origin.x() + bx,
                                                origin.y() + by,
                                                origin.z() + bz,
                                            ),
                                        );
                                        let value = brick.channel(fixture.material).uniform_value();
                                        assert!(
                                            value == Some(AIR) || value == Some(STONE),
                                            "{cube:?} may show nothing and has {value:?} in a brick"
                                        );
                                    }
                                }
                            }
                            if level > 0 {
                                let node = generator.coarse_node(cube, UNLIT).unwrap();
                                let cell = node.uniform_cell();
                                assert!(
                                    cell.is_some_and(|cell| cell.fill == 0
                                        || (cell.fill == 255 && cell.exposure == 0)),
                                    "{cube:?} may show nothing and its node is {node:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(
            pruned > 100 && kept > 100,
            "{pruned} pruned and {kept} kept"
        );
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

    use super::node_digest as digest;

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
