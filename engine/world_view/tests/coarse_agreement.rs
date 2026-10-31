//! How far the coarse cells a generator gives are from the coarse cells of the bricks it generates (`docs/lod-spec.md`, 38e).
//!
//! The generator's cells (`TerrainGenerator::coarse_node`) are worked out from the density field without generating the voxels. The exact ones are made the
//! other way: generate every brick of the cube and the bricks around it, make the node of each (`voxel_meshing::coarse_node_of_brick`), and make the nodes up
//! level by level (`CoarseNode::from_children`). The two are compared cell by cell; the numbers below are what that measured.

use std::collections::HashMap;

use block_models::BlockTable;
use coarse_cells::{CELLS_PER_NODE, CoarseNode, FULL_LIGHT};
use region_octree::BrickCube;
use terrain::test_support::{RollingHillsSettings, rolling_hills};
use terrain::{FieldTerrainGenerator, SurfaceLayers, TerrainGenerator};
use voxel_grid::test_support::StandardTestLayout;
use voxel_grid::{Brick, ChannelStorage};
use voxel_meshing::{BrickChannels, BrickNeighbourhood, coarse_node_of_brick};
use voxel_space::{BrickCoordinate, FaceDirection, LevelOfDetail, LocalVoxelIndex};

const AIR: u32 = 0;
const MATERIALS: SurfaceLayers = SurfaceLayers {
    air: AIR,
    deep: 1,
    subsurface: 2,
    surface: 3,
};

struct Exact {
    generator: FieldTerrainGenerator,
    fixture: StandardTestLayout,
    table: BlockTable,
    bricks: HashMap<BrickCoordinate, Brick>,
    nodes: HashMap<BrickCoordinate, CoarseNode>,
}

impl Exact {
    fn new(settings: RollingHillsSettings) -> Self {
        let fixture = StandardTestLayout::default();
        Self {
            generator: rolling_hills(settings, fixture.material, MATERIALS),
            table: BlockTable::of_cubes(&[false, true, true, true]),
            fixture,
            bricks: HashMap::new(),
            nodes: HashMap::new(),
        }
    }

    fn brick(&mut self, at: BrickCoordinate) -> &Brick {
        let (generator, layout) = (&self.generator, &self.fixture.layout);
        self.bricks
            .entry(at)
            .or_insert_with(|| generator.generate_brick(layout, at))
    }

    fn storage(&mut self, at: BrickCoordinate) -> ChannelStorage {
        let material = self.fixture.material;
        self.brick(at).channel(material).clone()
    }

    /// The node of one brick, with the bricks that touch it.
    fn brick_node(&mut self, at: BrickCoordinate) -> CoarseNode {
        if let Some(node) = self.nodes.get(&at) {
            return node.clone();
        }
        let center = self.storage(at);
        let neighbours: Vec<ChannelStorage> = FaceDirection::ALL
            .iter()
            .map(|face| {
                let step = face.offset();
                self.storage(at.checked_offset(step[0], step[1], step[2]).unwrap())
            })
            .collect();
        let node = coarse_node_of_brick(
            &BrickNeighbourhood {
                center: BrickChannels::blocks_only(&center),
                neighbours: std::array::from_fn(|index| {
                    Some(BrickChannels::blocks_only(&neighbours[index]))
                }),
            },
            &self.table,
            AIR,
        );
        self.nodes.insert(at, node.clone());
        node
    }

    /// The node of a cube of bricks, made from the voxels.
    fn node(&mut self, cube: BrickCube) -> CoarseNode {
        if cube.level() == 0 {
            return self.brick_node(cube.origin());
        }
        let half = 1i128 << (cube.level() - 1);
        let origin = cube.origin();
        let children: Vec<CoarseNode> = (0..8)
            .map(|octant| {
                let (dx, dy, dz) = (octant & 1, (octant >> 1) & 1, octant >> 2);
                let child = BrickCube::new(
                    origin
                        .checked_offset(dx * half, dy * half, dz * half)
                        .unwrap(),
                    cube.level() - 1,
                )
                .unwrap();
                self.node(child)
            })
            .collect();
        CoarseNode::from_children(
            std::array::from_fn(|index| &children[index]),
            LevelOfDetail::new(cube.level()).unwrap(),
            AIR,
        )
    }
}

/// How two nodes of the same cube compare.
#[derive(Debug, Default, Clone)]
struct Agreement {
    /// Cells where either has something in it.
    cells: u64,
    /// Of those, the cells that both draw or both leave.
    same_drawn: u64,
    /// Of the cells both draw, those with the same block.
    both_drawn: u64,
    same_block: u64,
    /// The sum of the differences of fill, of 255.
    fill_difference: u64,
    /// The cells where either has any surface (the ones whose block is seen), and of those the ones both draw and the ones with the same block.
    surface_cells: u64,
    surface_both_drawn: u64,
    surface_same_block: u64,
    surface_same_drawn: u64,
    /// How often the generator's block (first) was the exact one's (second), over the cells both draw with a surface.
    confusion: std::collections::BTreeMap<(u32, u32), u64>,
}

impl Agreement {
    fn of(coarse: &CoarseNode, exact: &CoarseNode) -> Self {
        let mut found = Self::default();
        for index in 0..CELLS_PER_NODE {
            let at = LocalVoxelIndex::from_index(index).unwrap();
            let (a, b) = (coarse.cell(at), exact.cell(at));
            if a.fill == 0 && b.fill == 0 {
                continue;
            }
            found.cells += 1;
            found.same_drawn += u64::from(a.is_drawn() == b.is_drawn());
            if a.is_drawn() && b.is_drawn() {
                found.both_drawn += 1;
                found.same_block += u64::from(a.block == b.block);
            }
            found.fill_difference += u64::from(a.fill.abs_diff(b.fill));
            if a.exposure > 0 || b.exposure > 0 {
                found.surface_cells += 1;
                found.surface_same_drawn += u64::from(a.is_drawn() == b.is_drawn());
                if a.is_drawn() && b.is_drawn() {
                    found.surface_both_drawn += 1;
                    found.surface_same_block += u64::from(a.block == b.block);
                    *found.confusion.entry((a.block, b.block)).or_default() += 1;
                }
            }
        }
        found
    }

    fn surface_drawn_agreement(&self) -> f64 {
        self.surface_same_drawn as f64 / self.surface_cells as f64
    }

    fn surface_block_agreement(&self) -> f64 {
        self.surface_same_block as f64 / self.surface_both_drawn as f64
    }

    fn add(&mut self, other: Self) {
        self.surface_cells += other.surface_cells;
        self.surface_both_drawn += other.surface_both_drawn;
        self.surface_same_block += other.surface_same_block;
        self.surface_same_drawn += other.surface_same_drawn;
        for (key, count) in other.confusion {
            *self.confusion.entry(key).or_default() += count;
        }
        self.cells += other.cells;
        self.same_drawn += other.same_drawn;
        self.both_drawn += other.both_drawn;
        self.same_block += other.same_block;
        self.fill_difference += other.fill_difference;
    }

    fn drawn_agreement(&self) -> f64 {
        self.same_drawn as f64 / self.cells as f64
    }

    fn mean_fill_difference(&self) -> f64 {
        self.fill_difference as f64 / self.cells as f64 / 255.0
    }
}

/// The cubes of a level that the surface of the world goes through, at a few places.
fn cubes_around_the_surface(level: u8, places: i128) -> Vec<BrickCube> {
    let width = 1i128 << level;
    let mut cubes = Vec::new();
    // Rows of cubes in height covering about 0 to 160 voxels (the surface is near 64, hills up to ~100 higher).
    let rows = ((160 / 32) / width).max(1);
    for row in 0..rows {
        for x in 0..places {
            for z in 0..places {
                cubes.push(
                    BrickCube::new(
                        BrickCoordinate::new((x * 3 + 1) * width, row * width, (z * 5 + 2) * width),
                        level,
                    )
                    .unwrap(),
                );
            }
        }
    }
    cubes
}

/// The agreement over a few cubes of each of the levels 1 to 4, for terrain with these settings: one entry for each level.
fn measure(settings: RollingHillsSettings) -> Vec<(u8, Agreement)> {
    let mut exact = Exact::new(settings);
    let mut found = Vec::new();
    for (level, places) in [(1u8, 2i128), (2, 2), (3, 1), (4, 1)] {
        let mut total = Agreement::default();
        for cube in cubes_around_the_surface(level, places) {
            let coarse = exact
                .generator
                .coarse_node(cube, FULL_LIGHT)
                .expect("the generator gives coarse nodes");
            total.add(Agreement::of(&coarse, &exact.node(cube)));
        }
        eprintln!(
            "level {level}: {} cells, drawn agree {:.4}, mean fill difference {:.4}; {} surface cells, drawn agree {:.4}, block agree {:.4} of {}; (coarse, exact) {:?}",
            total.cells,
            total.drawn_agreement(),
            total.mean_fill_difference(),
            total.surface_cells,
            total.surface_drawn_agreement(),
            total.surface_block_agreement(),
            total.surface_both_drawn,
            total.confusion
        );
        found.push((level, total));
    }
    found
}

#[test]
fn the_coarse_cells_of_hills_without_caves_are_close_to_those_of_the_bricks_at_every_level_from_1_to_4()
 {
    // Measured (2024, hills 40 high, 2 x 2 + 2 x 2 + 1 + 1 cubes around the surface; the thresholds are a little under what it found):
    //   level 1: cells drawn alike 100.00%, mean fill difference 0.01%, block of the surface cells alike 93%
    //   level 2: 100.00%, 0.02%, 88%
    //   level 3: 99.3%, 0.8%, 83%
    //   level 4: 99.1%, 0.9%, 83%
    let settings = RollingHillsSettings {
        cave_half_thickness: 0.0,
        ..RollingHillsSettings::for_one_metre_voxels(2024)
    };
    for (level, found) in measure(settings) {
        assert!(
            found.cells > 1_000,
            "level {level}: only {} cells",
            found.cells
        );
        assert!(found.drawn_agreement() > 0.96, "level {level}: {found:?}");
        assert!(
            found.mean_fill_difference() < 0.02,
            "level {level}: {found:?}"
        );
        assert!(
            found.surface_block_agreement() > 0.78,
            "level {level}: {found:?}"
        );
    }
}

#[test]
fn the_coarse_cells_of_hills_with_caves_are_as_full_as_those_of_the_bricks_and_the_exposed_faces_rule_prefers_the_stone_of_the_caves()
 {
    // Measured, as above, with the caves of the settings (sheets 32 voxels apart, a few voxels thick): how full a cell is and which cells are drawn
    // agree as closely as without caves (a little less at the coarser levels, where the generator's cells have lost the caves that are smaller than them), but the block does not: the walls of the caves have more surface than the top of the hill in a cell of 8 voxels or more, so the cells made
    // of the bricks are stone where the generator says grass (which is what is seen from outside).
    //   level 1: drawn alike 100.00%, fill difference 0.03%, surface block alike 78%
    //   level 2: 100.00%, 0.09%, 70%
    //   level 3: 98.2%, 2.3%, 62%
    //   level 4: 98.9%, 2.3%, 41%
    for (level, found) in measure(RollingHillsSettings::for_one_metre_voxels(2024)) {
        assert!(
            found.cells > 1_000,
            "level {level}: only {} cells",
            found.cells
        );
        assert!(found.drawn_agreement() > 0.95, "level {level}: {found:?}");
        assert!(
            found.mean_fill_difference() < 0.03,
            "level {level}: {found:?}"
        );
        // (The block agreement is only held above where it was measured to be, to notice if it falls; it is the open question of the rule.)
        assert!(
            found.surface_block_agreement() > 0.30,
            "level {level}: {found:?}"
        );
    }
}

#[test]
fn the_coarse_node_of_hills_from_the_generator_meshes_to_grass_on_top_and_far_fewer_quads_than_faces()
 {
    use voxel_space::FaceDirection;
    let settings = RollingHillsSettings {
        cave_half_thickness: 0.0,
        ..RollingHillsSettings::for_one_metre_voxels(2024)
    };
    let fixture = StandardTestLayout::default();
    let generator = rolling_hills(settings, fixture.material, MATERIALS);
    let cube = BrickCube::new(BrickCoordinate::new(8, 0, 8), 3).unwrap();
    let node = generator.coarse_node(cube, FULL_LIGHT).unwrap();
    let mesh = voxel_meshing::mesh_coarse_node(&node, &[None; 6]);
    let area = |quad: &voxel_meshing::Quad| u64::from(quad.width) * u64::from(quad.height);
    let top_area: u64 = mesh
        .quads
        .iter()
        .filter(|quad| quad.face == FaceDirection::PositiveY)
        .map(area)
        .sum();
    let grass_area: u64 = mesh
        .quads
        .iter()
        .filter(|quad| quad.face == FaceDirection::PositiveY && quad.material == MATERIALS.surface)
        .map(area)
        .sum();
    eprintln!(
        "MESH {} quads, top area {top_area}, grass {grass_area}",
        mesh.quads.len()
    );
    assert!(top_area > 500, "{top_area}");
    assert!(grass_area * 10 > top_area * 9, "{grass_area} of {top_area}");
    let faces: u64 = mesh.quads.iter().map(area).sum();
    assert!(
        (mesh.quads.len() as u64) * 3 < faces,
        "{} quads for {faces} faces",
        mesh.quads.len()
    );
}
