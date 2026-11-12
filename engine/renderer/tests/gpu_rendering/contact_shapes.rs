//! The sun's exact short ray through voxels that block the sun in part (`docs/specs/contact-trace-spec.md`, section 13): a slab, a stair, a post.
//!
//! A voxel of such a block is not a cube in the walk: the ray goes through the eighths of it that it crosses, and is blocked only if one of them is filled. These tests are of the
//! shader's walk against `world_view::sun_blocked_by_shapes`, on a brick of random cubes, slabs, stairs and posts whose occupancy is made by the real `BrickOccupancy::of` from a
//! real block table (so the chain from the table's shapes to the pixel is the one a game has), with the shapes of the table given to the renderer as the shell gives them:
//!
//! * the walk's answer (`ContactDebug`) at places on the faces of cubes, with slabs, stairs and posts in the way;
//! * at places on the tops of slabs and the treads of stairs, where the ray starts in a voxel that is itself partial;
//! * the pictures: the floor is dark from the very foot of a slab, and a slab does not shade what a cube would (the control).

use block_models::BlockTable;
use world_view::{SunCell, sun_blocked_by_shapes};

use super::contact_trace::{Random, SUNS, set_sun_for_the_walk, settled_at};
use super::*;

/// Air, a red cube, and a green block with the states: whole, bottom slab, top slab, a stair rising toward +x, a post in the middle.
pub(super) fn shaped_blocks() -> BlockTable {
    let air = BlockDefinition::air();
    let cube = BlockDefinition::cube([0.9, 0.1, 0.1]);
    let post = Shape::from_boxes(&[ShapeBox {
        min: [3, 0, 3],
        max: [5, 8, 5],
    }]);
    let rock = BlockDefinition::cube([0.1, 0.8, 0.1]).with_shapes(vec![
        Shape::full(),
        Shape::half(FaceDirection::NegativeY),
        Shape::half(FaceDirection::PositiveY),
        Shape::stair(FaceDirection::PositiveX),
        post,
        // A thin plate a quarter of the way up: it differs from row to row inside a half of the voxel, which no other shape here does.
        Shape::from_boxes(&[ShapeBox {
            min: [0, 1, 0],
            max: [8, 3, 8],
        }]),
    ]);
    BlockTable::from_definitions(&[Some(&air), Some(&cube), Some(&rock)])
}

const CUBE: u32 = 1;
const ROCK: u32 = 2;
const BOTTOM_SLAB: u32 = 1;
const TOP_SLAB: u32 = 2;
const STAIR: u32 = 3;
const POST: u32 = 4;
const SHELF: u32 = 5;

/// What is at a voxel of the brick: (block, state), air as (0, 0).
type Voxel = (u32, u32);

/// A brick of random voxels, in its inner part (2 to 29 along each axis): air, cubes, and the shapes of `ROCK`.
pub(super) struct ShapedSoup {
    pub(super) voxels: Vec<Voxel>,
}

impl ShapedSoup {
    pub(super) fn new(random: &mut Random, cubes: f64, shapes: f64) -> Self {
        let mut voxels = vec![(0, 0); 32 * 32 * 32];
        for z in 2..30usize {
            for y in 2..30usize {
                for x in 2..30usize {
                    let roll = random.unit();
                    voxels[(z * 32 + y) * 32 + x] = if roll < cubes {
                        (CUBE, 0)
                    } else if roll < cubes + shapes {
                        (
                            ROCK,
                            [BOTTOM_SLAB, TOP_SLAB, STAIR, POST, SHELF][random.below(5)],
                        )
                    } else {
                        (0, 0)
                    };
                }
            }
        }
        Self { voxels }
    }

    fn at(&self, voxel: [i128; 3]) -> Option<Voxel> {
        let inside = |axis: usize| {
            (0..32)
                .contains(&voxel[axis])
                .then_some(voxel[axis] as usize)
        };
        let (x, y, z) = (inside(0)?, inside(1)?, inside(2)?);
        Some(self.voxels[(z * 32 + y) * 32 + x])
    }

    /// What the walk finds in a voxel, from the shapes of the table and not from the occupancy the renderer was given: `None` outside the brick (not known).
    pub(super) fn cell(&self, table: &BlockTable, voxel: [i128; 3]) -> Option<SunCell> {
        let (block, state) = self.at(voxel)?;
        let model = table.model_of(block, state, 0);
        Some(if table.is_opaque_cube(model) {
            SunCell::Blocks
        } else if let Some(shape) = table.partial_sun_shape(model) {
            SunCell::Partial(table.shape_cells()[shape as usize])
        } else {
            SunCell::Clear
        })
    }

    fn is_air(&self, voxel: [i128; 3]) -> bool {
        self.at(voxel) == Some((0, 0))
    }

    fn blocks(&self) -> Vec<([u8; 3], u32, u32)> {
        let mut blocks = Vec::new();
        for z in 0..32usize {
            for y in 0..32usize {
                for x in 0..32usize {
                    let (block, state) = self.voxels[(z * 32 + y) * 32 + x];
                    if block != 0 {
                        blocks.push(([x as u8, y as u8, z as u8], block, state));
                    }
                }
            }
        }
        blocks
    }

    /// The occupancy as a mesher makes it, from the channels of the brick and the table.
    pub(super) fn occupancy(&self, table: &BlockTable) -> BrickOccupancy {
        let blocks: Vec<u32> = self.voxels.iter().map(|&(block, _)| block).collect();
        let states: Vec<u32> = self.voxels.iter().map(|&(_, state)| state).collect();
        let (blocks, states) = (
            ChannelStorage::from_values(&blocks),
            ChannelStorage::from_values(&states),
        );
        BrickOccupancy::of(
            &BrickChannels {
                blocks: &blocks,
                states: Some(&states),
                orientations: None,
                light: None,
            },
            table,
        )
    }

    /// A renderer that holds the soup, the way a game's shell tells it: the shapes of the table, the mesh, the occupancy.
    fn renderer(&self, context: &GraphicsContext, table: &BlockTable) -> VoxelRenderer {
        let mut renderer = new_renderer(context);
        renderer.set_sun_shapes(&table.shape_cells());
        renderer.upload_brick(
            ORIGIN_BRICK,
            &mesh_of_blocks(table, &self.blocks(), Some(0)),
        );
        renderer.set_occupancy(ORIGIN_BRICK, &self.occupancy(table));
        renderer
    }
}

/// A place on a face, and the camera that looks at it.
struct ShapedProbe {
    /// The voxel the walk starts in, where the place is in it (0 to 1 along each axis), and the way the face looks.
    front: [i128; 3],
    fraction: [f64; 3],
    face: FaceDirection,
    /// The two axes along the face.
    across: [usize; 2],
    eye: [f64; 3],
    target: [f64; 3],
}

/// The face of axis `axis` looking toward `sign`.
fn face_of(axis: usize, sign: i128) -> FaceDirection {
    FaceDirection::ALL[axis * 2 + usize::from(sign > 0)]
}

/// A random probe on a face of a cube that looks toward the sun, in the open (nothing within four cells in front of it, so that the camera, three voxels out, sees it), or
/// on the top of a slab or the tread of a stair (with nothing within four cells over it).
fn random_shaped_probe(
    random: &mut Random,
    soup: &ShapedSoup,
    toward_sun: [f64; 3],
    on_shapes: bool,
) -> ShapedProbe {
    loop {
        let voxel = [
            (2 + random.below(28)) as i128,
            (2 + random.below(28)) as i128,
            (2 + random.below(28)) as i128,
        ];
        let Some((block, state)) = soup.at(voxel) else {
            continue;
        };
        let (axis, sign, front, mut fraction);
        if on_shapes {
            // The top of a bottom slab, and the lower tread of a stair rising toward +x: a place in the middle of the voxel, a half up it.
            let tread = block == ROCK && (state == BOTTOM_SLAB || state == STAIR);
            if !tread || toward_sun[1] <= 0.05 {
                continue;
            }
            (axis, sign, front) = (1, 1, voxel);
            fraction = [0.0; 3];
            fraction[1] = 0.5;
            fraction[0] = if state == STAIR {
                0.1 + 0.3 * random.unit()
            } else {
                0.1 + 0.8 * random.unit()
            };
            fraction[2] = 0.1 + 0.8 * random.unit();
        } else {
            if block != CUBE {
                continue;
            }
            axis = random.below(3);
            sign = if random.below(2) == 0 { 1 } else { -1 };
            if toward_sun[axis] * sign as f64 <= 0.05 {
                continue;
            }
            let mut step = voxel;
            step[axis] += sign;
            front = step;
            fraction = [0.0; 3];
            fraction[axis] = if sign > 0 { 0.0 } else { 1.0 };
            for part in [(axis + 1) % 3, (axis + 2) % 3] {
                fraction[part] = 0.1 + 0.8 * random.unit();
            }
        }
        // Nothing in front of the face for four cells (from the voxel the place is in, for a tread).
        let clear = (1..=4).all(|cells| {
            let mut cell = front;
            cell[axis] += sign * cells;
            soup.is_air(cell)
        }) && (on_shapes || soup.is_air(front));
        if !clear {
            continue;
        }
        let target: [f64; 3] = std::array::from_fn(|part| front[part] as f64 + fraction[part]);
        let mut eye = target;
        eye[axis] += 3.0 * sign as f64;
        return ShapedProbe {
            front,
            fraction,
            face: face_of(axis, sign),
            across: [(axis + 1) % 3, (axis + 2) % 3],
            eye,
            target,
        };
    }
}

/// The reference's answer for the probe if it does not change when the place moves by up to a hundredth of a voxel across the face or the sun by a tenth of a thousandth.
fn exact_shaped_answer(
    soup: &ShapedSoup,
    table: &BlockTable,
    probe: &ShapedProbe,
    toward_sun: [f64; 3],
    cells: u32,
) -> Option<bool> {
    let front = VoxelCoordinate::new(probe.front[0], probe.front[1], probe.front[2]);
    let walk = |fraction: [f64; 3], sun: [f64; 3]| {
        sun_blocked_by_shapes(front, fraction, probe.face, sun, cells, |cell| {
            soup.cell(table, [cell.x(), cell.y(), cell.z()])
        })
    };
    let answer = walk(probe.fraction, toward_sun);
    for first in [-0.01, 0.0, 0.01] {
        for second in [-0.01, 0.0, 0.01] {
            let mut fraction = probe.fraction;
            fraction[probe.across[0]] += first;
            fraction[probe.across[1]] += second;
            if walk(fraction, toward_sun) != answer {
                return None;
            }
        }
    }
    for part in 0..3 {
        if toward_sun[part] == 0.0 {
            continue;
        }
        for scale in [1.0 - 1e-4, 1.0 + 1e-4] {
            let mut sun = toward_sun;
            sun[part] *= scale;
            if walk(probe.fraction, sun) != answer {
                return None;
            }
        }
    }
    Some(answer)
}

/// What the walk of the shader finds for the probe: whether the sun is blocked, as `ContactDebug::Answer` draws it (white: not; black: blocked).
fn shaped_shader_answer(renderer: &mut VoxelRenderer, probe: &ShapedProbe) -> bool {
    let camera = camera_aimed_at(probe.eye, probe.target);
    let image = settled_at(renderer, &camera, SIZE);
    let middle = pixel(&image, SIZE / 2, SIZE / 2);
    assert!(
        middle[..3] == [0, 0, 0] || middle[..3] == [255, 255, 255],
        "the answer is not black or white at the middle: {middle:?} (eye {:?}, target {:?})",
        probe.eye,
        probe.target
    );
    middle[0] == 0
}

/// Tries `probes` random probes for each sun and each number of cells and fails if the shader and the reference differ; returns how many were blocked and how many clear.
fn shaped_walk_against_the_reference(
    context: &GraphicsContext,
    on_shapes: bool,
    cells: &[u32],
    suns: &[[f32; 3]],
    probes: usize,
    seed: u64,
) -> (usize, usize) {
    let name = adapter_name(context);
    let table = shaped_blocks();
    let mut random = Random(seed);
    // (Many cubes and shapes: the ray meets something now and then in a few cells.)
    let soup = ShapedSoup::new(&mut random, 0.06, 0.2);
    let mut renderer = soup.renderer(context, &table);
    let (mut blocked, mut clear) = (0, 0);
    for &sun in suns {
        set_sun_for_the_walk(&mut renderer, sun);
        let toward_sun = Vec3::from_array(sun).normalize().to_array().map(f64::from);
        for &cell_count in cells {
            renderer.set_contact_debug(ContactDebug::Answer { cells: cell_count });
            let (mut found_blocked, mut found_clear, mut attempts) = (0, 0, 0);
            while found_blocked + found_clear < probes {
                attempts += 1;
                assert!(attempts < probes * 600, "{name}: too few probes are robust");
                let probe = random_shaped_probe(&mut random, &soup, toward_sun, on_shapes);
                let Some(exact) =
                    exact_shaped_answer(&soup, &table, &probe, toward_sun, cell_count)
                else {
                    continue;
                };
                let shader = shaped_shader_answer(&mut renderer, &probe);
                assert_eq!(
                    shader,
                    exact,
                    "{name}: sun {sun:?}, {cell_count} cells, front voxel {:?}, place {:?}, face {:?}: the shader says {}, the reference {}",
                    probe.front,
                    probe.fraction,
                    probe.face,
                    if shader { "blocked" } else { "clear" },
                    if exact { "blocked" } else { "clear" },
                );
                if exact {
                    found_blocked += 1;
                } else {
                    found_clear += 1;
                }
            }
            blocked += found_blocked;
            clear += found_clear;
        }
    }
    renderer.set_contact_debug(ContactDebug::Off);
    (blocked, clear)
}

#[test]
fn the_shader_walk_finds_what_the_reference_finds_at_the_faces_of_cubes_with_slabs_stairs_and_posts_in_the_way()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let (blocked, clear) =
            shaped_walk_against_the_reference(&context, false, &[1, 3, 8], &SUNS, 14, 0x51AB);
        assert!(
            blocked >= 100 && clear >= 100,
            "{name}: {blocked} blocked, {clear} clear"
        );
    }
}

#[test]
fn the_shader_walk_finds_what_the_reference_finds_from_the_top_of_a_slab_and_the_tread_of_a_stair()
{
    for context in contexts() {
        let name = adapter_name(&context);
        // (Only suns over the horizon reach a top; the starting voxel is partial, and the ray is walked through its eighths first.)
        let suns: Vec<[f32; 3]> = SUNS.iter().copied().filter(|sun| sun[1] > 0.05).collect();
        let (blocked, clear) =
            shaped_walk_against_the_reference(&context, true, &[1, 3, 8], &suns, 16, 0xC0DE);
        assert!(
            blocked >= 40 && clear >= 40,
            "{name}: {blocked} blocked, {clear} clear"
        );
    }
}

/// The scene of the pictures: a floor of cubes (its top at y = 1) with a wall of the block `ROCK` in the state given on it, at x 14 to 17 and z 14 to 17, one voxel high (so
/// half a voxel high, for a slab). The volume holds the floor and the wall as the mesher would make it.
fn wall_on_a_floor(state: u32) -> ShapedSoup {
    let mut soup = ShapedSoup {
        voxels: vec![(0, 0); 32 * 32 * 32],
    };
    for z in 0..32usize {
        for x in 0..32usize {
            soup.voxels[z * 1024 + x] = (CUBE, 0);
        }
    }
    for z in 14..18usize {
        for x in 14..18usize {
            soup.voxels[z * 1024 + 32 + x] = (ROCK, state);
        }
    }
    soup
}

#[test]
fn the_walk_through_shapes_is_a_shader_of_its_own_used_only_while_some_brick_has_voxels_that_block_in_part()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let table = shaped_blocks();
        let soup = wall_on_a_floor(BOTTOM_SLAB);
        let mut renderer = soup.renderer(&context, &table);
        let mut camera = camera_at(ORIGIN_BRICK, [8.0, 12.0, 30.0]);
        camera.pitch = -0.5;
        renderer.set_atmosphere(shadow_atmosphere(
            0,
            super::contact_trace::sun_at(30.0),
            true,
        ));
        let statistics = renderer.render_to_rgba(&camera, SIZE, SIZE).1;
        assert_eq!(
            (statistics.traced_windows, statistics.walked_shapes),
            (1, true),
            "{name}: a slab on the floor"
        );
        // The slabs dug out: the walk is the one through voxels only.
        let with_slabs = soup.occupancy(&table);
        assert!(matches!(with_slabs, BrickOccupancy::Shaped { .. }));
        renderer.set_occupancy(
            ORIGIN_BRICK,
            &super::contact_trace::occupancy_where(|_, y, _| y == 0),
        );
        let statistics = renderer.render_to_rgba(&camera, SIZE, SIZE).1;
        assert_eq!(
            (statistics.traced_windows, statistics.walked_shapes),
            (1, false),
            "{name}: a floor alone"
        );
        // And back.
        renderer.set_occupancy(ORIGIN_BRICK, &with_slabs);
        let statistics = renderer.render_to_rgba(&camera, SIZE, SIZE).1;
        assert_eq!(
            (statistics.traced_windows, statistics.walked_shapes),
            (1, true),
            "{name}: slabs again"
        );
        // Nothing is walked with the shadows off, shapes or no.
        renderer.set_atmosphere(shadow_atmosphere(
            0,
            super::contact_trace::sun_at(30.0),
            false,
        ));
        let statistics = renderer.render_to_rgba(&camera, SIZE, SIZE).1;
        assert_eq!(
            (statistics.traced_windows, statistics.walked_shapes),
            (0, false),
            "{name}: shadows off"
        );
    }
}

/// Elevations at which the shadow of a wall half a voxel high is long enough to be looked at in the strip at its foot.
const SLAB_ELEVATIONS: [f64; 5] = [5.0, 15.0, 30.0, 45.0, 60.0];

/// How many pixels of the strip at the foot of the wall (along the row z = 16, up to 0.05 short of where the shadow of a wall `wall_height` high truly ends) are lit, at the
/// elevations and heights of the camera of these tests; and how many there were in all.
fn lit_in_the_strip(renderer: &mut VoxelRenderer, wall_height: f64, name: &str) -> (usize, usize) {
    let (mut lit, mut counted) = (0, 0);
    for height in [20, 60] {
        for elevation in SLAB_ELEVATIONS {
            let end = wall_height / super::contact_trace::elevation_tangent(elevation);
            let (found_lit, found) = super::contact_trace::floor_row(
                renderer,
                elevation,
                height,
                16.0,
                0.004,
                (end - 0.05).min(1.5),
            );
            assert!(
                found >= 4,
                "{name}: camera {height} up, sun {elevation}: only {found} pixels in the strip"
            );
            lit += found_lit;
            counted += found;
        }
    }
    (lit, counted)
}

#[test]
fn the_floor_is_dark_from_the_very_foot_of_a_slab_and_not_when_the_volume_knows_nothing_of_slabs() {
    for context in contexts() {
        let name = adapter_name(&context);
        let table = shaped_blocks();
        let soup = wall_on_a_floor(BOTTOM_SLAB);
        let mut renderer = soup.renderer(&context, &table);
        let (lit, counted) = lit_in_the_strip(&mut renderer, 0.5, &name);
        assert_eq!(
            lit, 0,
            "{name}: {lit} of {counted} pixels at the foot of the slab are lit"
        );
        // The control: a volume that holds the floor and takes the slab for air (what the walk did before it knew shapes) leaves the strip, so that the test above can see a gap.
        renderer.set_occupancy(
            ORIGIN_BRICK,
            &super::contact_trace::occupancy_where(|_, y, _| y == 0),
        );
        let (lit_without, _) = lit_in_the_strip(&mut renderer, 0.5, &name);
        assert!(
            lit_without >= 40,
            "{name}: with the slab taken for air only {lit_without} pixels of the strip are lit, the test could not see a gap"
        );
    }
}

#[test]
fn a_slab_shades_the_floor_as_far_as_its_height_says_and_not_as_far_as_a_cube_would() {
    for context in contexts() {
        let name = adapter_name(&context);
        let table = shaped_blocks();
        let mut with_slab = wall_on_a_floor(BOTTOM_SLAB).renderer(&context, &table);
        // A sun at 30 degrees: a cube one voxel high shades the floor to 1.73 voxels from its foot, a slab half a voxel high to 0.87. Between them the floor is lit by the slab and
        // would be dark behind a cube (the map, which decides there, has no false shadows; the walk must not add one).
        let (lit, counted) =
            super::contact_trace::floor_row(&mut with_slab, 30.0, 20, 16.0, 1.0, 1.5);
        assert!(counted >= 20, "{name}: {counted}");
        assert_eq!(
            lit, counted,
            "{name}: the floor beyond the shadow of the slab is shaded ({lit} of {counted} lit)"
        );
        // The control: the same wall of cubes shades it.
        let mut cubes = wall_on_a_floor(BOTTOM_SLAB);
        for voxel in &mut cubes.voxels {
            if voxel.0 == ROCK {
                *voxel = (CUBE, 0);
            }
        }
        let mut with_cubes = cubes.renderer(&context, &table);
        let (lit, counted) =
            super::contact_trace::floor_row(&mut with_cubes, 30.0, 20, 16.0, 1.0, 1.5);
        assert!(counted >= 20, "{name}: {counted}");
        assert!(
            lit * 4 <= counted,
            "{name}: a wall of cubes leaves {lit} of {counted} lit, so the control cannot see the difference"
        );
    }
}
