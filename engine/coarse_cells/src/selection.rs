//! Which nodes of a grid are drawn for a viewer (`docs/lod-spec.md`, 3.3): one rule for terrain and for every ship, in the grid's own axes.
//!
//! A cube of bricks (a node: `BrickCube`, whose `level` is the detail level of its cells) is **fine enough** when its cells are small against its distance
//! from the viewer: a cell is `2^level` voxels across, and it is fine enough when that is at most `cell_pixels` times the size of a pixel at that
//! distance. A cube that is not fine enough is replaced by its eight halves, down to the bricks themselves (level 0), which are as fine as there is. So near
//! the viewer the nodes are small and far away they are large, within one grid, and a grid as large as a planet has a few nodes of each size, not one for
//! each brick.
//!
//! Only IEEE addition, subtraction, multiplication, division, comparison and square roots are used, so the answer is the same on every machine.

use region_octree::BrickCube;
use voxel_space::{BRICK_EDGE_LENGTH, BrickBox, BrickCoordinate, WorldPosition};

/// The coarsest detail level there is (`lod-spec.md`, decision 2): a cell of 2^40 voxels.
pub const MAXIMUM_DETAIL_LEVEL: u8 = 40;

/// Bricks and voxels: the length of an edge of a brick in `f64`.
const BRICK_EDGE: f64 = BRICK_EDGE_LENGTH as f64;

/// Half the diagonal of a cube of side 1: the radius of the sphere round it.
const HALF_DIAGONAL: f64 = 0.866_025_403_784_438_6;

/// How fine the drawing is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailSettings {
    /// How many pixels across a cell may be before it is split: 2 is a drawing in which no cell is seen as more than two pixels. A setting of the client
    /// (`.detail`).
    pub cell_pixels: f64,
    /// How big a pixel is as seen from the viewer, as a share of the distance: `2 * tan(field_of_view / 2) / viewport_height`. A node that is smaller than a
    /// pixel is not drawn at all. The camera knows it; it is a function of the field of view, so zooming makes it smaller.
    pub pixel_size: f64,
}

/// A cone toward which the detail is finer than elsewhere: where a player looks through a telescope. The nodes that touch the cone are chosen with
/// its `pixel_size`, which is smaller than the one of the settings, and all the others with the settings'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    /// The way it looks, a unit vector in the grid's axes.
    pub direction: [f64; 3],
    /// The cosine and the sine of the half angle of the cone, from the half of the field of view (and some more, for a view that moves).
    pub half_angle_cosine: f64,
    pub half_angle_sine: f64,
    /// The size of a pixel within the cone (see [`DetailSettings::pixel_size`]).
    pub pixel_size: f64,
}

/// What to select for.
#[derive(Clone, Copy)]
pub struct Selection<'a> {
    /// Where the viewer is, in the axes of the grid (for a ship, its pose's inverse has turned and moved it there).
    pub viewer: WorldPosition,
    pub settings: DetailSettings,
    pub zoom: Option<Zoom>,
    /// Nodes are never coarser than this detail level (a ship's cap; the default is [`MAXIMUM_DETAIL_LEVEL`]).
    pub coarsest_level: u8,
    /// Nodes whose nearest point is farther than this many voxels are not selected (the view distance of terrain, which has no end).
    pub maximum_distance: Option<f64>,
    /// Whether a node smaller than a pixel is left out. On unless a test turns it off.
    pub cull_sub_pixel: bool,
    /// Whether the cube may hold anything of the grid (a ship holds nothing outside its bounds, and a hole in it may be known): a cube it says no to is
    /// left out. Terrain says yes to every cube.
    pub may_hold: &'a dyn Fn(BrickCube) -> bool,
}

impl std::fmt::Debug for Selection<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Selection")
            .field("viewer", &self.viewer)
            .field("settings", &self.settings)
            .field("zoom", &self.zoom)
            .field("coarsest_level", &self.coarsest_level)
            .field("maximum_distance", &self.maximum_distance)
            .field("cull_sub_pixel", &self.cull_sub_pixel)
            .finish_non_exhaustive()
    }
}

fn anywhere(_: BrickCube) -> bool {
    true
}

impl<'a> Selection<'a> {
    /// A viewer with these settings, nothing special: no zoom, no cap, no distance limit, and every cube may hold something.
    pub fn new(viewer: WorldPosition, settings: DetailSettings) -> Self {
        Self {
            viewer,
            settings,
            zoom: None,
            coarsest_level: MAXIMUM_DETAIL_LEVEL,
            maximum_distance: None,
            cull_sub_pixel: true,
            may_hold: &anywhere,
        }
    }

    pub fn with_zoom(self, zoom: Zoom) -> Self {
        Self {
            zoom: Some(zoom),
            ..self
        }
    }

    /// Panics above [`MAXIMUM_DETAIL_LEVEL`].
    pub fn with_coarsest_level(self, level: u8) -> Self {
        assert!(
            level <= MAXIMUM_DETAIL_LEVEL,
            "detail level {level} is above the maximum"
        );
        Self {
            coarsest_level: level,
            ..self
        }
    }

    pub fn with_maximum_distance(self, voxels: f64) -> Self {
        Self {
            maximum_distance: Some(voxels),
            ..self
        }
    }

    pub fn without_culling(self) -> Self {
        Self {
            cull_sub_pixel: false,
            ..self
        }
    }

    pub fn with_may_hold(self, may_hold: &'a dyn Fn(BrickCube) -> bool) -> Self {
        Self { may_hold, ..self }
    }
}

/// Where a cube is from the viewer, in voxels along each axis: the low and the high end of the cube.
fn span(cube: BrickCube, viewer: WorldPosition) -> [(f64, f64); 3] {
    let origin = cube.origin();
    let edge = cube.edge_length() as f64 * BRICK_EDGE;
    let (brick, offset) = (viewer.brick(), viewer.offset());
    let along = |origin: i64, brick: i64, offset: f32| {
        let low = (origin - brick) as f64 * BRICK_EDGE - f64::from(offset);
        (low, low + edge)
    };
    [
        along(origin.x(), brick.x(), offset[0]),
        along(origin.y(), brick.y(), offset[1]),
        along(origin.z(), brick.z(), offset[2]),
    ]
}

/// The distance from the viewer to the nearest point of the cube, and the vector from the viewer to its middle.
fn distance_and_centre(span: &[(f64, f64); 3]) -> (f64, [f64; 3]) {
    let mut squared = 0.0;
    let mut centre = [0.0; 3];
    for (axis, &(low, high)) in span.iter().enumerate() {
        let gap = if low > 0.0 {
            low
        } else if high < 0.0 {
            -high
        } else {
            0.0
        };
        squared += gap * gap;
        centre[axis] = (low + high) * 0.5;
    }
    (squared.sqrt(), centre)
}

/// Whether the sphere of radius `radius` round `centre` (a vector from the cone's tip) touches the cone.
fn touches(zoom: &Zoom, centre: [f64; 3], radius: f64) -> bool {
    let along = |vector: [f64; 3]| {
        vector[0] * zoom.direction[0]
            + vector[1] * zoom.direction[1]
            + vector[2] * zoom.direction[2]
    };
    let length = |vector: [f64; 3]| {
        (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
    };
    if length(centre) <= radius {
        return true;
    }
    // The sphere touches the cone if its middle is inside the cone with its tip moved back by radius / sin(half angle).
    let back = radius / zoom.half_angle_sine.max(1.0e-12);
    let moved = [
        centre[0] + zoom.direction[0] * back,
        centre[1] + zoom.direction[1] * back,
        centre[2] + zoom.direction[2] * back,
    ];
    along(moved) >= length(moved) * zoom.half_angle_cosine
}

/// The nodes to draw: cubes of bricks of the grid, finer near the viewer and coarser far from it, in the order of a depth-first walk with the low half
/// first. `extent` is the box of bricks that holds the grid (a ship's bounds; for terrain the box that the view distance reaches). The cubes do not
/// overlap, and leave out only what [`Selection`] says to: what the grid cannot hold, what is too far, and what is under a pixel.
pub fn select_nodes(extent: BrickBox, selection: &Selection<'_>) -> Vec<BrickCube> {
    let widest = {
        let (low, high) = (extent.min(), extent.max());
        [high.x() - low.x(), high.y() - low.y(), high.z() - low.z()]
            .into_iter()
            .max()
            .expect("three axes")
            + 1
    };
    // The level at which a few cubes cover the extent, or the coarsest allowed if that is finer.
    let covering_level = (u64::BITS - (widest as u64 - 1).leading_zeros()) as u8;
    let root_level = covering_level.min(selection.coarsest_level);
    let mut chosen = Vec::new();
    for root in BrickCube::covering(extent, root_level) {
        descend(root, selection, &mut chosen);
    }
    chosen
}

fn descend(cube: BrickCube, selection: &Selection<'_>, chosen: &mut Vec<BrickCube>) {
    if !(selection.may_hold)(cube) {
        return;
    }
    let span = span(cube, selection.viewer);
    let (distance, centre) = distance_and_centre(&span);
    if selection
        .maximum_distance
        .is_some_and(|most| distance > most)
    {
        return;
    }
    let edge = cube.edge_length() as f64 * BRICK_EDGE;
    // The size of a pixel here: the zoom's if the node touches the cone and its pixel is smaller.
    let pixel_size = match &selection.zoom {
        Some(zoom)
            if zoom.pixel_size < selection.settings.pixel_size
                && touches(zoom, centre, edge * HALF_DIAGONAL) =>
        {
            zoom.pixel_size
        }
        _ => selection.settings.pixel_size,
    };
    if selection.cull_sub_pixel && edge < pixel_size * distance {
        return;
    }
    let cell_edge = (1u64 << cube.level()) as f64;
    let fine_enough = cell_edge <= selection.settings.cell_pixels * pixel_size * distance;
    // (A cube is never coarser than the cap: the walk starts at a level that is not, and only goes down.)
    if cube.level() == 0 || fine_enough {
        chosen.push(cube);
        return;
    }
    let half = 1i64 << (cube.level() - 1);
    let origin = cube.origin();
    for octant in 0..8i64 {
        let (dx, dy, dz) = (octant & 1, (octant >> 1) & 1, octant >> 2);
        let child = BrickCube::new(
            BrickCoordinate::new(
                origin.x() + dx * half,
                origin.y() + dy * half,
                origin.z() + dz * half,
            ),
            cube.level() - 1,
        )
        .expect("half of a cube is a cube");
        descend(child, selection, chosen);
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::{GridPose, LatticeRotation, Orientation};

    use super::*;

    /// A 1080-line view at 70 degrees, cells of 3 pixels: full detail to about 256 voxels.
    fn settings() -> DetailSettings {
        let pixel_size = 2.0 * 0.700_207_538_209_062_3 / 1080.0;
        DetailSettings {
            cell_pixels: 3.0,
            pixel_size,
        }
    }

    fn at(brick: [i64; 3], offset: [f32; 3]) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset)
    }

    fn box_of(low: [i64; 3], high: [i64; 3]) -> BrickBox {
        BrickBox::new(
            BrickCoordinate::new(low[0], low[1], low[2]),
            BrickCoordinate::new(high[0], high[1], high[2]),
        )
        .unwrap()
    }

    /// A grid as wide as a planet: 10^8 voxels across, about 3.1 million bricks, from the origin.
    const PLANET_BRICKS: i64 = 3_125_000;

    fn planet() -> BrickBox {
        box_of([0; 3], [PLANET_BRICKS - 1; 3])
    }

    fn volume(cubes: &[BrickCube]) -> u128 {
        cubes
            .iter()
            .map(|cube| u128::from(cube.edge_length()).pow(3))
            .sum()
    }

    fn counts_by_level(cubes: &[BrickCube]) -> Vec<usize> {
        let mut counts = vec![0usize; usize::from(MAXIMUM_DETAIL_LEVEL) + 1];
        for cube in cubes {
            counts[usize::from(cube.level())] += 1;
        }
        counts
    }

    #[test]
    fn a_viewer_close_to_a_small_grid_gets_every_brick_and_one_far_away_gets_the_whole_in_one() {
        let extent = box_of([0; 3], [1; 3]);
        let near = select_nodes(
            extent,
            &Selection::new(at([0; 3], [5.0; 3]), settings()).without_culling(),
        );
        assert_eq!(near.len(), 8);
        assert!(near.iter().all(|cube| cube.level() == 0));
        // 10^6 voxels away: a cell of 2 voxels is fine enough (the node has 64 voxels along an edge, of 2 each).
        let far = select_nodes(
            extent,
            &Selection::new(at([31_250, 0, 0], [0.0; 3]), settings()).without_culling(),
        );
        assert_eq!(far.len(), 1);
        assert_eq!(far[0].level(), 1);
        assert_eq!(far[0].origin(), BrickCoordinate::new(0, 0, 0));
    }

    #[test]
    fn a_cube_exactly_as_far_as_its_cells_need_is_fine_enough_and_one_a_little_nearer_is_not() {
        // A pixel of 1/256 of the distance and a cell of one pixel: a cell of 2 voxels (the cells of a cube of two bricks) is fine enough from 512 voxels.
        let exact = DetailSettings {
            cell_pixels: 1.0,
            pixel_size: 1.0 / 256.0,
        };
        let extent = box_of([0; 3], [1; 3]);
        let at_512 = select_nodes(
            extent,
            &Selection::new(at([-16, 0, 0], [0.0; 3]), exact).without_culling(),
        );
        assert_eq!(at_512.len(), 1);
        assert_eq!(at_512[0].level(), 1);
        let nearer = select_nodes(
            extent,
            &Selection::new(at([-16, 0, 0], [0.5, 0.0, 0.0]), exact).without_culling(),
        );
        assert_eq!(nearer.len(), 8);
    }

    #[test]
    fn the_cubes_tile_the_grid_with_no_gap_and_no_overlap() {
        let extent = box_of([-37, -2, 5], [70, 40, 60]);
        let selection =
            Selection::new(at([20, 12, 30], [3.5, 9.0, 21.0]), settings()).without_culling();
        let cubes = select_nodes(extent, &selection);
        // Every brick of the extent is in exactly one cube.
        let mut seen = std::collections::HashSet::new();
        for cube in &cubes {
            let edge = cube.edge_length() as i64;
            for x in 0..edge {
                for y in 0..edge {
                    for z in 0..edge {
                        let brick = BrickCoordinate::new(
                            cube.origin().x() + x,
                            cube.origin().y() + y,
                            cube.origin().z() + z,
                        );
                        assert!(seen.insert(brick), "{brick:?} is in two cubes");
                    }
                }
            }
        }
        for x in -37..=70 {
            for y in -2..=40 {
                for z in 5..=60 {
                    assert!(
                        seen.contains(&BrickCoordinate::new(x, y, z)),
                        "({x}, {y}, {z})"
                    );
                }
            }
        }
        // The cubes also cover bricks of the roots outside the extent; nothing else.
        assert!(volume(&cubes) as usize >= (70 + 37 + 1) * (40 + 2 + 1) * (60 - 5 + 1));
    }

    #[test]
    fn every_cube_chosen_is_fine_enough_and_the_cube_it_came_from_was_not() {
        let selection = Selection::new(at([1_000_000, 300_000, 20], [7.0, 1.0, 30.0]), settings())
            .without_culling();
        let cubes = select_nodes(planet(), &selection);
        let ratio = selection.settings.cell_pixels * selection.settings.pixel_size;
        for cube in &cubes {
            let (distance, _) = distance_and_centre(&span(*cube, selection.viewer));
            let cell = (1u64 << cube.level()) as f64;
            if cube.level() > 0 {
                assert!(cell <= ratio * distance, "{cube:?} is not fine enough");
            }
            // Its parent (a cube twice as wide) was not fine enough, or this is a root.
            if cube.level() < 22 {
                let parent = BrickCube::containing(cube.origin(), cube.level() + 1);
                let (parent_distance, _) = distance_and_centre(&span(parent, selection.viewer));
                assert!(
                    (1u64 << parent.level()) as f64 > ratio * parent_distance,
                    "{cube:?}: its parent was fine enough"
                );
            }
        }
    }

    /// A sphere of this radius (in bricks) with its middle at (radius, radius, radius): whether the cube touches its surface, with room to spare. What a
    /// planet whose inside is solid and whose outside is air shows.
    fn touches_the_surface_of_a_sphere(radius: i64) -> impl Fn(BrickCube) -> bool {
        move |cube: BrickCube| {
            let half = cube.edge_length() as f64 / 2.0;
            let centre = |component: i64| component as f64 + half - radius as f64;
            let (x, y, z) = (
                centre(cube.origin().x()),
                centre(cube.origin().y()),
                centre(cube.origin().z()),
            );
            let distance = (x * x + y * y + z * z).sqrt();
            (distance - radius as f64).abs() <= half * 1.732_050_807_568_877_2 + 1.0
        }
    }

    #[test]
    fn a_planet_has_some_thousands_of_cubes_in_all_finest_where_the_viewer_stands_and_a_solid_one_would_have_ten_times_as_many()
     {
        // A sphere 10^8 voxels across (radius 1.5 million bricks) and the viewer on its top, a few voxels in.
        let radius = PLANET_BRICKS / 2;
        let shell = touches_the_surface_of_a_sphere(radius);
        let extent = box_of([0; 3], [2 * radius - 1; 3]);
        let viewer = at([radius, 2 * radius - 1, radius], [16.0, 28.0, 16.0]);
        let selection = Selection::new(viewer, settings()).with_may_hold(&shell);
        let started = std::time::Instant::now();
        let cubes = select_nodes(extent, &selection);
        let elapsed = started.elapsed();
        let counts = counts_by_level(&cubes);
        eprintln!("PLANET {} cubes {counts:?} in {elapsed:?}", cubes.len());
        // The cube under the viewer's feet is a brick; the farthest cubes are very coarse.
        let under = cubes
            .iter()
            .find(|cube| cube.contains(BrickCoordinate::new(radius, 2 * radius - 1, radius)))
            .unwrap();
        assert_eq!(under.level(), 0);
        let coarsest = cubes.iter().map(|cube| cube.level()).max().unwrap();
        assert!(coarsest >= 17, "{coarsest}");
        // Of each size, a few hundred to a few thousand; in all some tens of thousands.
        assert!(counts.iter().all(|&count| count < 6_000), "{counts:?}");
        assert!(cubes.len() < 60_000, "{}", cubes.len());
        // The cubes are the same on every machine: a digest of the whole list (FNV-1a over each origin and level), pinned (Windows and Linux agree).
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        for cube in &cubes {
            let origin = cube.origin();
            for value in [
                origin.x() as u64,
                origin.y() as u64,
                origin.z() as u64,
                u64::from(cube.level()),
            ] {
                for byte in value.to_le_bytes() {
                    state ^= u64::from(byte);
                    state = state.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        assert_eq!(state, 9_369_750_438_262_538_815);
        // The solid planet (nothing to say that the inside is not seen) has far more.
        let solid = select_nodes(extent, &Selection::new(viewer, settings()));
        assert!(
            solid.len() > 5 * cubes.len(),
            "{} against {}",
            solid.len(),
            cubes.len()
        );
    }

    #[test]
    fn a_cap_forbids_cubes_coarser_than_it_and_the_grid_is_covered_by_cubes_of_the_cap() {
        let extent = box_of([0; 3], [63; 3]);
        let selection = Selection::new(at([1_000_000, 0, 0], [0.0; 3]), settings())
            .without_culling()
            .with_coarsest_level(3);
        let cubes = select_nodes(extent, &selection);
        assert!(cubes.iter().all(|cube| cube.level() <= 3));
        // Far away, so every cube is fine enough where it is: the cubes of the cap itself, 8 along each edge.
        assert_eq!(cubes.len(), 8 * 8 * 8);
        assert!(cubes.iter().all(|cube| cube.level() == 3));
        // A cap of 0 is every brick.
        let bricks = select_nodes(
            box_of([0; 3], [7; 3]),
            &Selection::new(at([1_000_000, 0, 0], [0.0; 3]), settings())
                .without_culling()
                .with_coarsest_level(0),
        );
        assert_eq!(bricks.len(), 512);
    }

    #[test]
    fn a_node_under_a_pixel_is_not_drawn_whatever_the_cap_and_a_larger_one_is() {
        // One brick (32 voxels) a million voxels away is 3.2e-5 of the distance across: a pixel is 1.3e-3.
        let extent = box_of([0; 3], [0; 3]);
        let far = Selection::new(at([31_250, 0, 0], [0.0; 3]), settings());
        assert!(select_nodes(extent, &far).is_empty());
        assert!(select_nodes(extent, &far.with_coarsest_level(0)).is_empty());
        // The same brick 10,000 voxels away is 3.2e-3: more than a pixel.
        let near = Selection::new(at([312, 0, 0], [0.0; 3]), settings());
        assert_eq!(select_nodes(extent, &near).len(), 1);
        // And a planet at that distance is drawn.
        let planet_far = Selection::new(at([31_250_000, 0, 0], [0.0; 3]), settings());
        assert!(!select_nodes(planet(), &planet_far).is_empty());
    }

    #[test]
    fn nodes_beyond_the_distance_limit_are_left_out() {
        let selection = Selection::new(at([0; 3], [0.0; 3]), settings())
            .without_culling()
            .with_maximum_distance(500.0);
        let cubes = select_nodes(box_of([-64; 3], [63; 3]), &selection);
        assert!(!cubes.is_empty());
        for cube in &cubes {
            let (distance, _) = distance_and_centre(&span(*cube, selection.viewer));
            assert!(distance <= 500.0, "{cube:?} is {distance} away");
        }
        // Without the limit there are more.
        let unlimited = select_nodes(
            box_of([-64; 3], [63; 3]),
            &Selection::new(at([0; 3], [0.0; 3]), settings()).without_culling(),
        );
        assert!(unlimited.len() > cubes.len());
    }

    #[test]
    fn cubes_the_grid_cannot_hold_are_left_out() {
        // A ship that has bricks only in the lower half of its box.
        let holds = |cube: BrickCube| cube.origin().y() < 4;
        let selection = Selection::new(at([2, 20, 2], [0.0; 3]), settings())
            .without_culling()
            .with_may_hold(&holds);
        let cubes = select_nodes(box_of([0; 3], [7; 3]), &selection);
        assert!(!cubes.is_empty());
        assert!(cubes.iter().all(|cube| cube.origin().y() < 4));
        let all = select_nodes(
            box_of([0; 3], [7; 3]),
            &Selection::new(at([2, 20, 2], [0.0; 3]), settings()).without_culling(),
        );
        assert!(all.len() > cubes.len());
    }

    fn zoom_toward_positive_x() -> Zoom {
        // A telescope of ten times the zoom with a view of 6 degrees all round (the cone is a little wider than the view).
        let half = 3.0f64.to_radians();
        Zoom {
            direction: [1.0, 0.0, 0.0],
            half_angle_cosine: libm::cos(half),
            half_angle_sine: libm::sin(half),
            pixel_size: settings().pixel_size / 10.0,
        }
    }

    /// A world with a surface: bricks from y = -4 to 3, a million voxels each way along the ground (the shape of terrain with a view distance).
    fn slab() -> (BrickBox, impl Fn(BrickCube) -> bool) {
        (
            box_of([-16_384, -4, -16_384], [16_383, 3, 16_383]),
            |cube: BrickCube| {
                cube.origin().y() < 4 && cube.origin().y() + cube.edge_length() as i64 > -4
            },
        )
    }

    #[test]
    fn a_zoom_refines_what_is_in_the_cone_and_leaves_the_rest_as_it_was() {
        let (extent, holds) = slab();
        let viewer = at([0, 4, 0], [3.0, 3.0, 3.0]);
        let base = Selection::new(viewer, settings()).with_may_hold(&holds);
        let plain = select_nodes(extent, &base);
        let zoomed = select_nodes(extent, &base.with_zoom(zoom_toward_positive_x()));
        eprintln!("ZOOM {} cubes plain, {} zoomed", plain.len(), zoomed.len());
        assert!(zoomed.len() > plain.len());
        // Every zoomed cube is inside a plain one that is as coarse or coarser: zooming only refines.
        let plain_set: std::collections::HashSet<BrickCube> = plain.iter().copied().collect();
        let level_of_plain_cube_holding = |brick: BrickCoordinate| {
            (0..=MAXIMUM_DETAIL_LEVEL)
                .find(|&level| plain_set.contains(&BrickCube::containing(brick, level)))
        };
        let mut finer = 0;
        for cube in &zoomed {
            let level = level_of_plain_cube_holding(cube.origin())
                .unwrap_or_else(|| panic!("{cube:?} is in no plain cube"));
            assert!(cube.level() <= level, "{cube:?}");
            finer += usize::from(cube.level() < level);
        }
        assert!(finer > 0);
        // A cube that was chosen without the zoom and that the cone does not touch is chosen with it: the cone changes only what it touches. (A cube
        // behind the viewer, or off to the side, is such a cube.)
        let zoom = zoom_toward_positive_x();
        let zoomed_set: std::collections::HashSet<BrickCube> = zoomed.iter().copied().collect();
        let mut untouched = 0;
        for cube in &plain {
            let (_, centre) = distance_and_centre(&span(*cube, viewer));
            let radius = cube.edge_length() as f64 * 32.0 * HALF_DIAGONAL;
            if !touches(&zoom, centre, radius) {
                untouched += 1;
                assert!(
                    zoomed_set.contains(cube),
                    "{cube:?} changed outside the cone"
                );
            }
        }
        assert!(untouched > 1_000, "{untouched} cubes outside the cone");
        // And the cone is where the cubes came to be finer: the nearest cube to the axis, 5 km out, is finer with the zoom.
        let on_axis = |cubes: &[BrickCube]| {
            cubes
                .iter()
                .filter(|cube| {
                    let (_, centre) = distance_and_centre(&span(**cube, viewer));
                    centre[0] > 4_000.0
                        && centre[0] < 6_000.0
                        && centre[1].abs() < 300.0
                        && centre[2].abs() < 300.0
                })
                .map(|cube| cube.level())
                .min()
        };
        assert!(
            on_axis(&zoomed) < on_axis(&plain),
            "{:?} against {:?}",
            on_axis(&zoomed),
            on_axis(&plain)
        );
    }

    #[test]
    fn a_zoom_that_is_not_finer_changes_nothing() {
        let (extent, holds) = slab();
        let viewer = at([0, 4, 0], [3.0, 3.0, 3.0]);
        let base = Selection::new(viewer, settings()).with_may_hold(&holds);
        let plain = select_nodes(extent, &base);
        // The same pixel, and a pixel twice as big (a wider view): neither is a reason to change anything, least of all to coarsen.
        for factor in [1.0, 2.0] {
            let zoom = Zoom {
                pixel_size: settings().pixel_size * factor,
                ..zoom_toward_positive_x()
            };
            assert_eq!(
                select_nodes(extent, &base.with_zoom(zoom)),
                plain,
                "{factor}"
            );
        }
    }

    #[test]
    fn the_same_inputs_give_the_same_cubes_in_the_same_order() {
        let selection = Selection::new(at([123_456, 7, 99_999], [1.5, 2.5, 3.5]), settings());
        assert_eq!(
            select_nodes(planet(), &selection),
            select_nodes(planet(), &selection)
        );
    }

    #[test]
    fn a_ship_turned_and_placed_anywhere_gets_the_cubes_of_the_point_in_its_own_axes_that_the_viewer_is_at()
     {
        // The ship is turned a quarter about the vertical axis, and its origin is at (100, 0, 0) bricks in the world; a viewer at a place in the world is
        // at the place of the ship's axes that the inverse of its pose gives, and the nodes are those of an unturned ship with the viewer there.
        let turn = Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0);
        let pose = GridPose::new(at([100, 0, 0], [0.0; 3]), turn);
        // A point 40 voxels along the ship's x axis and 10 up: in the world, turned.
        let local = at([1, 0, 0], [8.0, 10.0, 0.0]);
        let world = pose.local_to_world(local).unwrap();
        let found = pose.world_to_local(world).unwrap();
        let extent = box_of([0; 3], [15; 3]);
        let from_world = select_nodes(extent, &Selection::new(found, settings()).without_culling());
        let direct = select_nodes(extent, &Selection::new(local, settings()).without_culling());
        assert_eq!(from_world, direct);
        // And the same through a rotation of the lattice (the turn is exact, so are the cubes).
        let lattice = LatticeRotation::all()[5].orientation();
        let pose = GridPose::new(at([-7, 3, 11], [4.0, 5.0, 6.0]), lattice);
        let world = pose.local_to_world(local).unwrap();
        let found = pose.world_to_local(world).unwrap();
        assert_eq!(
            select_nodes(extent, &Selection::new(found, settings()).without_culling()),
            direct
        );
    }

    #[test]
    fn the_viewer_inside_a_cube_has_a_distance_of_nothing_and_a_cube_is_split_to_the_bricks() {
        let cubes = select_nodes(
            box_of([0; 3], [3; 3]),
            &Selection::new(at([1, 1, 1], [16.0; 3]), settings()).without_culling(),
        );
        let under = cubes
            .iter()
            .find(|cube| cube.contains(BrickCoordinate::new(1, 1, 1)))
            .unwrap();
        assert_eq!(under.level(), 0);
    }
}
