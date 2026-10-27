//! The coarse nodes of a planet (`docs/planet-spec.md`, section 3): what its ground looks like from far away, made from columns.
//!
//! A node of detail level `n` is 32 x 32 x 32 cells of `2^n` voxels. Asking the ground about the 4 x 4 x 4 points of every cell of a surface node (as
//! `coarse.rs` does for flat terrain) costs far too much for a planet, whose ground takes many noise octaves to say anything. Instead each of the 32 x 32
//! columns of cells along the axis the node is nearest to the direction of (the same axis for all of it) is walked from an estimate of where the ground ends
//! in it, asking the field at the middle of each cell until the cells are clearly air on one side and clearly ground on the other: a few questions for a
//! column instead of a thousand. The fill of a cell comes from the value of the field at its middle and the width of the cell across the surface (a cell
//! is a cube, so the distance from its middle to its face along the surface's normal is half the sum of the normal's components times the cell's edge), and the
//! blocks and surfaces from the fills as for flat terrain, but with the layers looking outward along the axis instead of up.
//!
//! **This is an approximation**, like all coarse cells: a cell that the surface goes through at a slant gets the fill of a flat cut, and a column that the
//! surface folds over (terrain steeper than the angle of the column to the surface, which the hills of a planet reach near the corners of the cube) can be
//! walked to the wrong place. How far from asking about every cell that is has been measured in the tests below.

use coarse_cells::{CELLS_PER_NODE, CoarseCell, CoarseNode, NODE_EDGE};
use region_octree::BrickCube;
use voxel_space::{LevelOfDetail, VoxelBox};

use crate::coarse::coarse_node_from_fills;
use crate::density::DensityField;
use crate::generator::{SURFACE_LAYER_DEPTH, TerrainMaterials};
use crate::planet::PlanetGround;

/// How many times the height of the ground is asked for to guess where the first column of a row ends: the first guess is the ground without hills, and each
/// time the column is run to the end that the last height gave and the height there asked for again.
const FIRST_GUESS_ITERATIONS: usize = 2;

/// Cells along the axis, beyond the node on each side, that a column keeps (the layers of the surface look a few cells outward, and the fills of the cells
/// beside the node are needed for the surface it shows).
const MARGIN: i128 = 6;

/// Cells in a column as it is kept.
const COLUMN_LENGTH: usize = NODE_EDGE + 2 * MARGIN as usize;

/// Columns across the node and the ring of them round it.
const COLUMNS_ACROSS: i128 = NODE_EDGE as i128 + 2;

/// The most cells a walk goes on for, either way, before it gives up and takes the rest of the column as it is going (air outward, ground inward).
const WALK_LIMIT: usize = 64;

impl PlanetGround {
    /// How far from the centre, along `axis`, the ground ends on one side of the planet, on the line parallel to the axis through the point `lateral` (the
    /// other two coordinates, in the order of [`lateral_axes`]), at the detail of `footprint`: a guess, from running the line to the end that the height at the
    /// last end gives. `None` if the line misses the ground there.
    fn guess_surface_along(
        &self,
        axis: usize,
        lateral: [f64; 2],
        outward_positive: bool,
        footprint: LevelOfDetail,
    ) -> Option<f64> {
        let [first, second] = lateral_axes(axis);
        let lateral_squared = lateral[0] * lateral[0] + lateral[1] * lateral[1];
        let sign = if outward_positive { 1.0 } else { -1.0 };
        let mut height = 0.0;
        for step in 0..=FIRST_GUESS_ITERATIONS {
            let reach = self.radius() + height;
            let squared = reach * reach - lateral_squared;
            if squared <= 0.0 {
                return None;
            }
            let along = squared.sqrt();
            if step == FIRST_GUESS_ITERATIONS {
                return Some(along);
            }
            let mut position = [0.0; 3];
            position[axis] = sign * along;
            position[first] = lateral[0];
            position[second] = lateral[1];
            height = self.height(position, reach, footprint);
        }
        None
    }
}

/// The two axes across the one given, in the order the columns of a node are numbered.
fn lateral_axes(axis: usize) -> [usize; 2] {
    [(axis + 1) % 3, (axis + 2) % 3]
}

/// The fill of a cell whose middle has the value `depth` in the field, when the surface is `width` across the cell along its normal: 0 for a cell that is clearly
/// air, 255 for one that is clearly ground, and between 1 and 254 for one the surface goes through.
fn fill_from_depth(depth: f64, width: f64) -> u8 {
    if depth <= -width / 2.0 {
        0
    } else if depth >= width / 2.0 {
        255
    } else {
        (((0.5 + depth / width) * 255.0).round() as u8).clamp(1, 254)
    }
}

/// Where a node is and which way its columns run.
struct Frame {
    detail: LevelOfDetail,
    /// Voxels along an edge of a cell.
    edge: i128,
    /// The axis the columns run along, and whether outward is toward its positive end.
    axis: usize,
    outward_positive: bool,
    /// The voxel at the lowest corner of the node, along each axis.
    origin: [i128; 3],
}

impl Frame {
    fn of(cube: BrickCube) -> Option<Self> {
        let level = cube.level();
        if level == 0 {
            return None;
        }
        let detail = LevelOfDetail::new(level)?;
        let edge = 1i128 << level;
        let corner = cube.origin().origin();
        let origin = [corner.x(), corner.y(), corner.z()];
        let width = (NODE_EDGE as i128 * edge) as f64;
        let middle = origin.map(|component| component as f64 + 0.5 * width);
        let axis = (0..3)
            .max_by(|&a, &b| middle[a].abs().total_cmp(&middle[b].abs()))
            .expect("there are three axes");
        Some(Self {
            detail,
            edge,
            axis,
            outward_positive: middle[axis] >= 0.0,
            origin,
        })
    }

    /// The step along the axis, from a cell to the one outward of it.
    fn outward(&self) -> i128 {
        if self.outward_positive { 1 } else { -1 }
    }

    /// Whether the cells of the columns reach the plane through the centre of the planet across the axis, so that a column can have the far side of the planet in
    /// it as well as the near: then the ground is not "up to a cell and nothing beyond".
    fn reaches_the_centre_plane(&self) -> bool {
        let first_outward = if self.outward_positive {
            self.origin[self.axis] - MARGIN * self.edge
        } else {
            -(self.origin[self.axis] + (NODE_EDGE as i128 + MARGIN) * self.edge)
        };
        first_outward <= 0
    }

    /// The position across the axis of the middle of the column with these numbers, counted from the node's (-1 and 32 are in the ring round the node).
    fn lateral(&self, i: i128, j: i128) -> [f64; 2] {
        let [first, second] = lateral_axes(self.axis);
        [
            self.origin[first] as f64 + (i as f64 + 0.5) * self.edge as f64,
            self.origin[second] as f64 + (j as f64 + 0.5) * self.edge as f64,
        ]
    }

    /// The number along the axis of the cell where the first guess puts the end of the ground in the column at `lateral`; `None` if the line misses the ground.
    fn guess_cell(&self, ground: &PlanetGround, lateral: [f64; 2]) -> Option<i128> {
        let end =
            ground.guess_surface_along(self.axis, lateral, self.outward_positive, self.detail)?;
        let signed = if self.outward_positive { end } else { -end };
        Some(((signed - self.origin[self.axis] as f64) / self.edge as f64).floor() as i128)
    }

    /// How full the cell with number `m` along the axis in the column at `lateral` is, from the value of the field at its middle.
    fn cell_fill(&self, ground: &PlanetGround, lateral: [f64; 2], m: i128) -> u8 {
        let [first, second] = lateral_axes(self.axis);
        let mut position = [0.0; 3];
        position[self.axis] = self.origin[self.axis] as f64 + (m as f64 + 0.5) * self.edge as f64;
        position[first] = lateral[0];
        position[second] = lateral[1];
        let distance =
            (position[0] * position[0] + position[1] * position[1] + position[2] * position[2])
                .sqrt();
        if distance == 0.0 {
            // The very centre of the planet.
            return 255;
        }
        let depth = ground.radius() + ground.height(position, distance, self.detail) - distance;
        let across = (position[0].abs() + position[1].abs() + position[2].abs()) / distance;
        fill_from_depth(depth, self.edge as f64 * across)
    }

    /// Fills `column` (the cells -MARGIN.. along the axis) by walking from cell `start`: asking about the cells outward until one is clearly air, and inward until
    /// one is clearly ground; the cells beyond are taken to be as those are. Returns a cell in the middle of what was walked, to start the next column from.
    fn walk(
        &self,
        ground: &PlanetGround,
        lateral: [f64; 2],
        start: i128,
        column: &mut [u8],
    ) -> i128 {
        let (lowest, highest) = (-MARGIN, NODE_EDGE as i128 + MARGIN - 1);
        let slot = |m: i128| (m + MARGIN) as usize;
        let outward = self.outward();
        let start = start.clamp(lowest, highest);
        column[slot(start)] = self.cell_fill(ground, lateral, start);

        let mut outer = start;
        let mut steps = 0;
        while column[slot(outer)] > 0 && steps < WALK_LIMIT {
            let next = outer + outward;
            if !(lowest..=highest).contains(&next) {
                break;
            }
            column[slot(next)] = self.cell_fill(ground, lateral, next);
            outer = next;
            steps += 1;
        }
        let mut inner = start;
        steps = 0;
        while column[slot(inner)] < 255 && steps < WALK_LIMIT {
            let next = inner - outward;
            if !(lowest..=highest).contains(&next) {
                break;
            }
            column[slot(next)] = self.cell_fill(ground, lateral, next);
            inner = next;
            steps += 1;
        }
        for m in lowest..=highest {
            if (m - outer) * outward > 0 {
                column[slot(m)] = 0;
            } else if (inner - m) * outward > 0 {
                column[slot(m)] = 255;
            }
        }
        (outer + inner).div_euclid(2)
    }
}

/// The columns of a node: how full every cell is, found a column at a time.
pub(crate) struct NodeColumns {
    frame: Frame,
    /// The cells of the 34 x 34 columns, `COLUMN_LENGTH` for each, numbered `(second + 1) * 34 + first + 1` by the numbers of the column across the axis.
    fills: Vec<u8>,
}

impl NodeColumns {
    /// The columns of the node of `cube` by walking each, or by asking about every cell where the node reaches the middle of the planet; `None` for a cube of
    /// level 0 (a brick).
    pub(crate) fn new(ground: &PlanetGround, cube: BrickCube) -> Option<Self> {
        let frame = Frame::of(cube)?;
        if frame.reaches_the_centre_plane() {
            return Some(Self::asking_every_cell_of(frame, ground));
        }
        let mut fills = vec![0u8; (COLUMNS_ACROSS * COLUMNS_ACROSS) as usize * COLUMN_LENGTH];
        for j in -1..=NODE_EDGE as i128 {
            // A column ends about where its neighbour's does; the first of a row is guessed.
            let mut previous: Option<i128> = None;
            for i in -1..=NODE_EDGE as i128 {
                let lateral = frame.lateral(i, j);
                let start = previous.or_else(|| frame.guess_cell(ground, lateral));
                let at = (((j + 1) * COLUMNS_ACROSS + i + 1) as usize) * COLUMN_LENGTH;
                previous = start.map(|start| {
                    frame.walk(ground, lateral, start, &mut fills[at..at + COLUMN_LENGTH])
                });
            }
        }
        Some(Self { frame, fills })
    }

    /// The same as [`NodeColumns::new`] by asking about every cell of every column, which is the fills as the field gives them (the tests compare the walk with it).
    #[cfg(test)]
    pub(crate) fn asking_every_cell(ground: &PlanetGround, cube: BrickCube) -> Option<Self> {
        Some(Self::asking_every_cell_of(Frame::of(cube)?, ground))
    }

    fn asking_every_cell_of(frame: Frame, ground: &PlanetGround) -> Self {
        let mut fills =
            Vec::with_capacity((COLUMNS_ACROSS * COLUMNS_ACROSS) as usize * COLUMN_LENGTH);
        for j in -1..=NODE_EDGE as i128 {
            for i in -1..=NODE_EDGE as i128 {
                let lateral = frame.lateral(i, j);
                for m in -MARGIN..NODE_EDGE as i128 + MARGIN {
                    fills.push(frame.cell_fill(ground, lateral, m));
                }
            }
        }
        Self { frame, fills }
    }

    /// How full the cell with these numbers (counted from the node's, and in the ring of columns round it) is, 0 to 255; 0 for a cell in a column beyond that
    /// ring, and the nearest cell that is kept for one beyond the margin.
    pub(crate) fn fill(&self, cell: [i128; 3]) -> u8 {
        let [first, second] = lateral_axes(self.frame.axis);
        let (i, j) = (cell[first], cell[second]);
        let range = -1..=NODE_EDGE as i128;
        if !range.contains(&i) || !range.contains(&j) {
            return 0;
        }
        let m = cell[self.frame.axis].clamp(-MARGIN, NODE_EDGE as i128 + MARGIN - 1);
        self.fills
            [((j + 1) * COLUMNS_ACROSS + i + 1) as usize * COLUMN_LENGTH + (m + MARGIN) as usize]
    }

    /// The fill of every cell of the node, x fastest.
    pub(crate) fn fills(&self) -> Vec<u8> {
        let mut fills = Vec::with_capacity(CELLS_PER_NODE);
        for z in 0..NODE_EDGE as i128 {
            for y in 0..NODE_EDGE as i128 {
                for x in 0..NODE_EDGE as i128 {
                    fills.push(self.fill([x, y, z]));
                }
            }
        }
        fills
    }

    /// The step to the cell outward of a cell.
    fn outward(&self) -> [i128; 3] {
        let mut step = [0; 3];
        step[self.frame.axis] = self.frame.outward();
        step
    }
}

/// The coarse node of a cube of bricks of a planet's ground, or `None` for a cube of level 0 (a brick, which is generated).
pub(crate) fn coarse_node_of_planet(
    ground: &PlanetGround,
    materials: TerrainMaterials,
    cube: BrickCube,
    unlit: u8,
) -> Option<CoarseNode> {
    if cube.level() == 0 {
        return None;
    }
    let detail = LevelOfDetail::new(cube.level())?;
    let edge = 1i128 << cube.level();
    let corner = cube.origin().origin();
    let width = NODE_EDGE as i128 * edge;
    // The reach of what the surface of a cell looks at: the cells beside it, and over it as far as the layers of the surface go.
    let reach = edge * (1 + (SURFACE_LAYER_DEPTH as i128 + edge - 1) / edge);
    let region = |margin: i128| {
        let low = corner.checked_offset(-margin, -margin, -margin)?;
        let high =
            corner.checked_offset(width + margin - 1, width + margin - 1, width + margin - 1)?;
        VoxelBox::new(low, high)
    };
    // Air all through; or ground all through, with ground all round it and over it.
    if let Some(node) = region(0)
        && ground.bounds(&node, detail).upper() <= 0.0
    {
        return Some(CoarseNode::uniform(CoarseCell::empty(materials.air, unlit)));
    }
    if let Some(around) = region(reach)
        && ground.bounds(&around, detail).lower() > 0.0
    {
        return Some(CoarseNode::uniform(CoarseCell {
            block: materials.stone,
            fill: 255,
            light: unlit,
            exposure: 0,
        }));
    }

    let columns = NodeColumns::new(ground, cube)?;
    let fills = columns.fills();
    if fills.iter().all(|&fill| fill == 0) {
        return Some(CoarseNode::uniform(CoarseCell::empty(materials.air, unlit)));
    }
    Some(coarse_node_from_fills(
        &fills,
        &|cell| columns.fill(cell),
        columns.outward(),
        columns.frame.edge,
        materials,
        unlit,
    ))
}

#[cfg(test)]
mod tests {
    use voxel_space::{BrickCoordinate, LocalVoxelIndex, VoxelCoordinate};

    use super::*;
    use crate::planet::test_fixtures::{MATERIALS, ball, small_planet};
    use crate::planet::{PlanetSettings, planet};

    const UNLIT: u8 = 9;

    /// A cube of the given level that holds the surface of the planet in the direction given (a direction, not a unit vector), if `radius` is where the surface is.
    fn cube_at_the_surface(direction: [f64; 3], radius: f64, level: u8) -> BrickCube {
        let length = (direction[0] * direction[0]
            + direction[1] * direction[1]
            + direction[2] * direction[2])
            .sqrt();
        let edge = 1i128 << level;
        let brick =
            direction.map(|component| ((component / length * radius) / 32.0).floor() as i128);
        let aligned = brick.map(|component| component.div_euclid(edge) * edge);
        BrickCube::new(
            BrickCoordinate::new(aligned[0], aligned[1], aligned[2]),
            level,
        )
        .unwrap()
    }

    /// Directions to look at: down each axis, and not along them, near the edges between faces of the cube and the corners.
    const DIRECTIONS: [[f64; 3]; 15] = [
        [0.02, 1.0, -0.03],
        [1.0, 0.1, 0.05],
        [0.07, -0.04, -1.0],
        [1.0, 0.86, 0.2],
        [0.3, 1.0, 0.55],
        [1.0, 0.93, 0.88],
        [-1.0, 0.6, -0.3],
        [0.5, -1.0, 0.6],
        [-0.82, -0.9, 1.0],
        [0.1, 1.0, 0.3],
        [1.0, -0.2, 0.6],
        [-0.15, -0.05, -1.0],
        [0.9, 1.0, -0.4],
        [-0.4, 0.7, -1.0],
        [0.2, -1.0, -0.7],
    ];

    fn real_planet() -> (PlanetGround, f64) {
        let settings = PlanetSettings::one_percent_of_earth(2024);
        (PlanetGround::new(&settings), settings.radius)
    }

    fn planet_node(settings: PlanetSettings, cube: BrickCube) -> CoarseNode {
        let fixture = voxel_grid::test_support::StandardTestLayout::default();
        let generator = planet(settings, fixture.material, MATERIALS);
        crate::TerrainGenerator::coarse_node(&generator, cube, UNLIT).unwrap()
    }

    // ---- symmetry ----

    /// The cells of a node moved by a symmetry of the cube: new axis `i` is old axis `permutation[i]`, flipped if `flips[i]`.
    fn cells_moved(
        node: &CoarseNode,
        permutation: [usize; 3],
        flips: [bool; 3],
    ) -> Vec<CoarseCell> {
        let mut moved = Vec::with_capacity(CELLS_PER_NODE);
        for z in 0..32usize {
            for y in 0..32usize {
                for x in 0..32usize {
                    let new = [x, y, z];
                    let mut old = [0usize; 3];
                    for axis in 0..3 {
                        let coordinate = new[axis];
                        old[permutation[axis]] = if flips[axis] {
                            31 - coordinate
                        } else {
                            coordinate
                        };
                    }
                    moved.push(node.cell(LocalVoxelIndex::new(
                        old[0] as u8,
                        old[1] as u8,
                        old[2] as u8,
                    )));
                }
            }
        }
        moved
    }

    /// The cube that a symmetry of the planet moves a cube to.
    fn cube_moved(cube: BrickCube, permutation: [usize; 3], flips: [bool; 3]) -> BrickCube {
        let old = [cube.origin().x(), cube.origin().y(), cube.origin().z()];
        let edge = cube.edge_length() as i128;
        let mut new = [0i128; 3];
        for axis in 0..3 {
            let coordinate = old[permutation[axis]];
            new[axis] = if flips[axis] {
                -coordinate - edge
            } else {
                coordinate
            };
        }
        BrickCube::new(BrickCoordinate::new(new[0], new[1], new[2]), cube.level()).unwrap()
    }

    /// Whether the middle of the cube is as far along two axes as the greatest.
    fn axes_tie(cube: BrickCube) -> bool {
        let width = (cube.edge_length() * 32) as i128;
        let corner = cube.origin().origin();
        let mut distances = [corner.x(), corner.y(), corner.z()].map(|c| (2 * c + width).abs());
        distances.sort();
        distances[2] == distances[1]
    }

    #[test]
    fn a_ball_looks_the_same_from_every_side_in_its_nodes() {
        // The cells of the node at the +x pole are those of the node at the +y pole turned, and so on for all the symmetries of the cube: the layers
        // look outward, not up, and nothing in the columns knows which axis it runs along.
        let settings = ball(500.0);
        let symmetries = [
            ([1, 2, 0], [false, false, false]),
            ([2, 0, 1], [false, false, false]),
            ([1, 0, 2], [false, false, false]),
            ([0, 1, 2], [true, false, false]),
            ([0, 1, 2], [false, true, true]),
            ([2, 1, 0], [true, true, false]),
            ([1, 2, 0], [false, true, false]),
        ];
        let mut surface_nodes = 0;
        for level in 1..=4u8 {
            for direction in DIRECTIONS {
                let cube = cube_at_the_surface(direction, 500.0, level);
                if axes_tie(cube) {
                    // Which axis the columns run along is not defined by the cube's symmetry when two axes are equally far; the node is as good, but not the same.
                    continue;
                }
                let node = planet_node(settings, cube);
                surface_nodes += usize::from(node.uniform_cell().is_none());
                for (permutation, flips) in symmetries {
                    let moved = planet_node(settings, cube_moved(cube, permutation, flips));
                    assert_eq!(
                        moved.cells().collect::<Vec<_>>(),
                        cells_moved(&node, permutation, flips),
                        "{cube:?} moved by {permutation:?} {flips:?}"
                    );
                }
            }
        }
        assert!(
            surface_nodes >= 30,
            "only {surface_nodes} of the nodes had the surface in them"
        );
    }

    #[test]
    fn the_outside_of_a_ball_is_grass_then_dirt_then_stone_at_every_pole() {
        // Looking down from outside along each axis at the middle of the node over a pole: the first cell that is drawn is grass, and what is under it is dirt
        // while less than the depth of the surface layer (4 voxels, so two cells of 2 voxels and one of 4 under the grass, which is almost full) of ground is
        // over it, then stone: the layers go inward from the surface, whichever way
        // the axis points.
        let settings = ball(500.0);
        for (level, dirt_cells) in [(1u8, 2usize), (2, 1), (3, 0)] {
            let edge = 1i128 << level;
            for axis in 0..3 {
                for outward_positive in [true, false] {
                    let mut direction = [0.01, 0.02, 0.03];
                    direction[axis] = if outward_positive { 1.0 } else { -1.0 };
                    let cube = cube_at_the_surface(direction, 500.0, level);
                    let node = planet_node(settings, cube);
                    // The column of cells through the voxel at the pole, from outside in.
                    let origin = cube.origin().origin();
                    let origin = [origin.x(), origin.y(), origin.z()];
                    let cell_of =
                        |voxel: i128, axis: usize| (voxel - origin[axis]).div_euclid(edge);
                    let mut position = [0i128; 3];
                    position[axis] = if outward_positive { 499 } else { -500 };
                    let cells = [0, 1, 2].map(|each| cell_of(position[each], each));
                    let outward = if outward_positive { 1 } else { -1 };
                    let mut column = Vec::new();
                    // From two cells outside to five inside.
                    for inward in -2..6 {
                        let mut at = cells;
                        at[axis] = cells[axis] - outward * inward;
                        if at.iter().all(|&n| (0..32).contains(&n)) {
                            column.push(node.cell(LocalVoxelIndex::new(
                                at[0] as u8,
                                at[1] as u8,
                                at[2] as u8,
                            )));
                        }
                    }
                    let drawn: Vec<_> = column.iter().filter(|cell| cell.is_drawn()).collect();
                    assert!(drawn.len() >= 3, "level {level} axis {axis}: {column:?}");
                    let blocks: Vec<u32> =
                        drawn.iter().take(2 + dirt_cells).map(|c| c.block).collect();
                    let mut expected = vec![MATERIALS.grass];
                    expected.extend(vec![MATERIALS.dirt; dirt_cells]);
                    expected.push(MATERIALS.stone);
                    assert_eq!(
                        blocks, expected,
                        "level {level}, axis {axis}, outward positive {outward_positive}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_node_with_its_far_corner_just_inside_the_surface_has_dirt_in_that_corner() {
        // The bounds say "ground all through" for a node whose farthest corner is under the surface, but the surface is a voxel or two beyond that corner
        // and the cell there is within the depth of the surface layer: dirt, not stone. (The bounds look a cell's reach beyond the node for this.)
        let cube = BrickCube::new(BrickCoordinate::new(0, 12, 0), 1).unwrap();
        let farthest = (63.5f64 * 63.5 * 2.0 + 447.5 * 447.5).sqrt();
        let node = planet_node(ball(farthest + 1.5), cube);
        assert_ne!(
            node.uniform_cell().map(|cell| cell.block),
            Some(MATERIALS.stone)
        );
        let corner = node.cell(LocalVoxelIndex::new(31, 31, 31));
        assert!(
            corner.block == MATERIALS.dirt || corner.block == MATERIALS.grass,
            "{corner:?}"
        );
        // The cell in the middle of the node, deep in the ground, is stone.
        assert_eq!(
            node.cell(LocalVoxelIndex::new(10, 5, 10)).block,
            MATERIALS.stone
        );
    }

    #[test]
    fn the_surface_a_few_cells_over_the_top_of_a_node_is_seen_by_the_layers_under_it() {
        // The ground reaches 3 voxels over the top of this node, at its middle: the top row of cells has 1.5 cells of ground over it, so dirt. The layers look
        // that far beyond the node, which needs the fills of the cells beyond (the margin of a column), not the last cell of the node again.
        let cube = BrickCube::new(BrickCoordinate::new(0, 12, 0), 1).unwrap();
        let node = planet_node(ball(451.0), cube);
        let top_row = node.cell(LocalVoxelIndex::new(0, 31, 0));
        assert_eq!(top_row.block, MATERIALS.dirt, "{top_row:?}");
        assert!(top_row.fill == 255);
        // And the one under it has three cells of ground over it: stone.
        assert_eq!(
            node.cell(LocalVoxelIndex::new(0, 29, 0)).block,
            MATERIALS.stone
        );
    }

    #[test]
    fn the_first_guess_of_a_row_is_within_a_few_cells_of_the_surface() {
        // The guess is only where the walk starts, and the walk finds the surface from anywhere, but a poor guess is a long walk (and one that gives up after 64
        // cells): the real planet's hills move the surface by up to a hundred voxels.
        let (ground, radius) = real_planet();
        let mut checked = 0;
        for level in [1u8, 3, 5] {
            for direction in &DIRECTIONS[..6] {
                let cube = cube_at_the_surface(*direction, radius, level);
                let frame = Frame::of(cube).unwrap();
                for j in [-1i128, 7, 20, 32] {
                    let lateral = frame.lateral(-1, j);
                    let Some(guess) = frame.guess_cell(&ground, lateral) else {
                        continue;
                    };
                    let outermost = (-MARGIN..NODE_EDGE as i128 + MARGIN)
                        .filter(|&m| frame.cell_fill(&ground, lateral, m) > 0)
                        .max_by_key(|&m| m * frame.outward());
                    // (A column whose surface is outside what a column keeps has nothing to compare.)
                    let Some(outermost) = outermost
                        .filter(|&m| (-MARGIN + 1..NODE_EDGE as i128 + MARGIN - 1).contains(&m))
                    else {
                        continue;
                    };
                    assert!(
                        (guess - outermost).abs() <= 6,
                        "level {level}, column {j}: guessed cell {guess}, the surface is in {outermost}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 20, "{checked}");
    }

    // ---- shortcuts ----

    #[test]
    fn nodes_far_from_the_ground_or_deep_in_it_are_one_cell() {
        let fixture = voxel_grid::test_support::StandardTestLayout::default();
        let generator = planet(small_planet(4), fixture.material, MATERIALS);
        let cube = |origin: [i128; 3], level: u8| {
            BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), level).unwrap()
        };
        let node = |origin, level| {
            crate::TerrainGenerator::coarse_node(&generator, cube(origin, level), UNLIT)
        };
        assert_eq!(
            node([-8, 24, -8], 3).unwrap().uniform_cell(),
            Some(CoarseCell::empty(MATERIALS.air, UNLIT))
        );
        assert_eq!(
            node([-4, -4, -4], 2).unwrap().uniform_cell(),
            Some(CoarseCell {
                block: MATERIALS.stone,
                fill: 255,
                light: UNLIT,
                exposure: 0
            })
        );
        // A brick is generated, not summarised.
        assert!(node([0, 15, 0], 0).is_none());
    }

    #[test]
    fn the_nodes_at_the_centre_of_a_planet_are_balls_of_cells_and_mirror_images_of_each_other() {
        // The cells are anchored at the origin, which is the centre of the planet: eight nodes meet there, each holding an eighth of a ball, and the columns of
        // each have the far side of the planet beside them (they reach the plane through the centre).
        let settings = ball(300.0);
        let node_at = |origin: [i128; 3]| {
            let cube =
                BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), 5).unwrap();
            planet_node(settings, cube)
        };
        let octant = node_at([0, 0, 0]);
        // 300 voxels of radius in cells of 32 voxels: 9.4 cells.
        let mut solid = 0;
        for z in 0..32u8 {
            for y in 0..32u8 {
                for x in 0..32u8 {
                    let cell = octant.cell(LocalVoxelIndex::new(x, y, z));
                    let middle = [x, y, z].map(|n| 32.0 * f64::from(n) + 16.0);
                    let distance =
                        (middle[0] * middle[0] + middle[1] * middle[1] + middle[2] * middle[2])
                            .sqrt();
                    // A cell is at most 28 voxels from its middle to a corner, so 28 voxels either side of the radius it is clearly in or out.
                    if distance < 300.0 - 28.0 {
                        assert_eq!(cell.fill, 255, "{x}, {y}, {z}");
                        solid += 1;
                    } else if distance > 300.0 + 28.0 {
                        assert_eq!(cell.fill, 0, "{x}, {y}, {z}");
                    }
                }
            }
        }
        // An eighth of (4/3) pi (272/32)^3: more than 300 cells.
        assert!(solid > 300, "{solid}");
        for flips in [
            [true, false, false],
            [false, true, false],
            [false, false, true],
            [true, true, true],
        ] {
            let origin = flips.map(|flip| if flip { -32 } else { 0 });
            assert_eq!(
                node_at(origin).cells().collect::<Vec<_>>(),
                cells_moved(&octant, [0, 1, 2], flips),
                "{flips:?}"
            );
        }
    }

    // ---- against asking about every cell ----

    #[test]
    fn walking_the_columns_gives_what_asking_about_every_cell_gives() {
        // The small planet has steeper hills than the real one (a fifth of the wavelength tall at every octave, twice the real ratio), and shows what
        // happens where a column is folded over by the surface; the real one is the one that matters.
        let measure = |ground: &PlanetGround, radius: f64, level: u8, directions: &[[f64; 3]]| {
            let (mut surface, mut different) = (0usize, 0usize);
            for &direction in directions {
                let cube = cube_at_the_surface(direction, radius, level);
                let walked = NodeColumns::new(ground, cube).unwrap();
                let every = NodeColumns::asking_every_cell(ground, cube).unwrap();
                surface += every
                    .fills
                    .iter()
                    .filter(|&&fill| fill != 0 && fill != 255)
                    .count();
                different += walked
                    .fills
                    .iter()
                    .zip(&every.fills)
                    .filter(|(a, b)| a != b)
                    .count();
            }
            (surface, different)
        };
        let (mut surface, mut different) = (0, 0);
        for seed in [1u64, 2] {
            let ground = PlanetGround::new(&small_planet(seed));
            for level in 1..=5u8 {
                let (cells, differ) = measure(&ground, 500.0, level, &DIRECTIONS);
                surface += cells;
                different += differ;
            }
        }
        eprintln!(
            "MEASURE small planet: {different} of {surface} cells with a surface in them differ"
        );
        assert!(
            different * 30 < surface,
            "small planet: {different} of {surface} cells differ from asking about each"
        );

        let (ground, radius) = real_planet();
        for level in 1..=7u8 {
            let (surface, different) = measure(&ground, radius, level, &DIRECTIONS);
            eprintln!("MEASURE real planet level {level}: {different} of {surface} differ");
            assert!(surface > 8000, "level {level}: {surface}");
            assert!(
                different * 200 < surface,
                "real planet level {level}: {different} of {surface} cells differ from asking about each"
            );
        }
    }

    /// The share of the voxels of a cell (on a lattice of up to 4 x 4 x 4) that are ground at full detail.
    fn share_that_is_ground(ground: &PlanetGround, cube: BrickCube, cell: [i128; 3]) -> f64 {
        let edge = 1i128 << cube.level();
        let origin = cube.origin().origin();
        let samples = edge.min(4);
        let mut count = 0;
        for z in 0..samples {
            for y in 0..samples {
                for x in 0..samples {
                    let at = |origin: i128, cell: i128, sample: i128| {
                        origin + cell * edge + (2 * sample + 1) * edge / (2 * samples)
                    };
                    let position = VoxelCoordinate::new(
                        at(origin.x(), cell[0], x),
                        at(origin.y(), cell[1], y),
                        at(origin.z(), cell[2], z),
                    );
                    if ground.sample(position, LevelOfDetail::FULL_DETAIL) > 0.0 {
                        count += 1;
                    }
                }
            }
        }
        f64::from(count) / (samples * samples * samples) as f64
    }

    #[test]
    fn the_cells_that_are_drawn_are_the_cells_that_most_of_the_voxels_under_them_make() {
        // How often a cell with a surface in or beside it is drawn (half full or more) when most of its voxels at full detail are not, or the other way round.
        // A cell is one question asked of the field (at its middle) where the voxels are many, so some are wrong: the measured shares are 17% at level 1 (cells
        // of 2 voxels, where one voxel is an eighth) and 3% to 6% above, and the limits have a margin over them. A cell that is wrong has nearly half
        // its voxels the other way; a mistake of the columns (a cell out of place) would be wrong by far more than these limits allow.
        let (real, real_radius) = real_planet();
        let small = PlanetGround::new(&small_planet(1));
        let planets = [
            ("small", &small, 500.0, 1..=4u8),
            ("real", &real, real_radius, 2..=6u8),
        ];
        for (name, ground, radius, levels) in planets {
            for level in levels {
                let (mut surface_cells, mut wrong) = (0usize, 0usize);
                for direction in &DIRECTIONS[..3] {
                    let cube = cube_at_the_surface(*direction, radius, level);
                    let fills = NodeColumns::new(ground, cube).unwrap().fills();
                    for (index, &fill) in fills.iter().enumerate() {
                        if fill == 0 || fill == 255 {
                            continue;
                        }
                        let cell = [
                            (index % 32) as i128,
                            ((index / 32) % 32) as i128,
                            (index / 1024) as i128,
                        ];
                        surface_cells += 1;
                        let most_are_ground = share_that_is_ground(ground, cube, cell) >= 0.5;
                        wrong += usize::from((fill >= 128) != most_are_ground);
                    }
                }
                eprintln!("MEASURE {name} level {level}: {wrong} of {surface_cells} wrong");
                let limit = if level == 1 { 0.22 } else { 0.08 };
                assert!(
                    surface_cells > 1000,
                    "{name} level {level}: {surface_cells}"
                );
                assert!(
                    (wrong as f64) < limit * surface_cells as f64,
                    "{name} level {level}: {wrong} of {surface_cells} cells with a surface in them are drawn wrongly"
                );
            }
        }
    }

    #[test]
    fn the_columns_fill_the_cells_as_asking_each_cell_in_three_dimensions_does() {
        // The other way of making the fills (`coarse.rs`, a lattice of 4 x 4 x 4 points in each cell) agrees about whether a cell is drawn in the great
        // majority of the cells with a surface in or beside them; the two are different approximations, each some 5% from the voxels.
        let mut compared = 0usize;
        let mut drawn_differently = 0usize;
        for seed in [1u64, 2] {
            let ground = PlanetGround::new(&small_planet(seed));
            for level in 1..=4u8 {
                for direction in DIRECTIONS {
                    let cube = cube_at_the_surface(direction, 500.0, level);
                    let from_columns = NodeColumns::new(&ground, cube).unwrap().fills();
                    let asked = crate::coarse::fills_of_field_cell_by_cell(&ground, cube).unwrap();
                    for (&a, &b) in from_columns.iter().zip(&asked) {
                        if a == 0 && b == 0 || a == 255 && b == 255 {
                            continue;
                        }
                        compared += 1;
                        drawn_differently += usize::from((a >= 128) != (b >= 128));
                    }
                }
            }
        }
        eprintln!("MEASURE compared {compared}, drawn differently {drawn_differently}");
        assert!(compared > 20_000, "{compared}");
        assert!(
            drawn_differently * 10 < compared,
            "{drawn_differently} of {compared} cells with a surface in or beside them are drawn differently"
        );
    }

    /// Like the bricks of a generator, its coarse nodes are not to change once there is a recording of them (a replay must draw the same picture): a digest of
    /// the nodes at the surface at several levels and directions, one at a corner where three faces meet, and one of the nodes at the centre.
    #[test]
    fn the_coarse_nodes_of_the_planet_are_pinned() {
        let settings = PlanetSettings::one_percent_of_earth(2024);
        let centre = |origin: [i128; 3]| {
            BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), 5).unwrap()
        };
        let cubes = [
            (settings, cube_at_the_surface(DIRECTIONS[0], 63_710.0, 3)),
            (settings, cube_at_the_surface(DIRECTIONS[1], 63_710.0, 6)),
            (settings, cube_at_the_surface(DIRECTIONS[5], 63_710.0, 4)),
            (settings, cube_at_the_surface(DIRECTIONS[6], 63_710.0, 9)),
            (settings, cube_at_the_surface(DIRECTIONS[8], 63_710.0, 2)),
            // The nodes at the centre of a small planet, which the columns reach the far side from.
            (small_planet(7), centre([0, 0, 0])),
            (small_planet(7), centre([-32, 0, -32])),
        ];
        let observed: Vec<u64> = cubes
            .iter()
            .map(|&(settings, cube)| {
                let node = planet_node(settings, cube);
                assert_eq!(node.uniform_cell(), None, "{cube:?} should have a surface");
                crate::coarse::node_digest(&node)
            })
            .collect();
        eprintln!("OBSERVED {observed:?}");
        assert_eq!(observed, PINNED_NODES);
    }

    const PINNED_NODES: [u64; 7] = [
        9684880242900089711,
        1459940348098846842,
        11714404734402302814,
        17631117748389673254,
        11780572891706905334,
        7197261811063683271,
        16210751029125396683,
    ];
}
