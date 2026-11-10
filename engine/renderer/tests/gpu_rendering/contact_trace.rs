//! The sun's exact short ray through the voxels (`docs/specs/contact-trace-spec.md`).
//!
//! Where a wall meets the floor the shadow map leaves a lit strip a texel or two wide (its bias keeps a thin floor from shading itself), and the same strip leaks the sun into a
//! closed cave. The walk of the sun's ray through the voxels, for the first few cells from a face, decides there instead. These tests are of that walk, against the exact
//! answers:
//!
//! * the pictures: the floor is dark from the very foot of a wall at every height of the sun and in every cascade of the map, a closed room of rock is dark inside, and nothing is
//!   darkened that should not be;
//! * the walk itself: the shader's answer for a place on a face (`ContactDebug`, which draws nothing but the answer) against `world_view::sun_blocked`, on random soups of voxels
//!   and suns with parts of exactly zero, in the terrain and in a grid that is turned.

use super::*;

const BIG: u32 = 256;

/// The occupancy of a brick whose voxels `blocks` says block the sun.
fn occupancy_where(blocks: impl Fn(usize, usize, usize) -> bool) -> BrickOccupancy {
    let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
    for z in 0..32usize {
        for y in 0..32usize {
            for x in 0..32usize {
                if blocks(x, y, z) {
                    words[z * 32 + y] |= 1 << x;
                }
            }
        }
    }
    BrickOccupancy::from_words(words)
}

/// The scene of the shadow tests with the floor and a block of four cells on it, and the voxel volume told of its brick.
fn block_scene_renderer(context: &GraphicsContext) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
    renderer.set_occupancy(
        ORIGIN_BRICK,
        &occupancy_where(|x, y, z| ShadowScene::Block.cell(x, y, z).is_some()),
    );
    renderer
}

/// A picture `size` pixels across, once a frame after it repeats it exactly (see `settled`).
fn settled_at(renderer: &mut VoxelRenderer, camera: &Camera, size: u32) -> Vec<u8> {
    let mut previous = renderer.render_to_rgba(camera, size, size).0;
    for _ in 0..30 {
        let next = renderer.render_to_rgba(camera, size, size).0;
        if next == previous {
            return next;
        }
        previous = next;
    }
    panic!("the picture never settled");
}

/// The picture of the scene with the shadows on and the same with them off, for the sun in this direction.
fn shadows_on_and_off(
    renderer: &mut VoxelRenderer,
    sun: [f32; 3],
    camera: &Camera,
) -> (Vec<u8>, Vec<u8>) {
    renderer.set_atmosphere(shadow_atmosphere(0, sun, false));
    let off = settled_at(renderer, camera, BIG);
    renderer.set_atmosphere(shadow_atmosphere(0, sun, true));
    let on = settled_at(renderer, camera, BIG);
    (on, off)
}

fn big_brightness(image: &[u8], x: u32, y: u32) -> f64 {
    let start = ((y * BIG + x) * 4) as usize;
    linear_brightness([
        image[start],
        image[start + 1],
        image[start + 2],
        image[start + 3],
    ])
}

/// The direction to a sun this many degrees over the +x horizon.
#[allow(clippy::disallowed_methods)] // a test of cosmetics, not simulation
fn sun_at(elevation: f64) -> [f32; 3] {
    let radians = elevation.to_radians();
    [radians.cos() as f32, radians.sin() as f32, 0.0]
}

/// The tangent of an elevation in degrees.
#[allow(clippy::disallowed_methods)] // a test of cosmetics, not simulation
fn elevation_tangent(elevation: f64) -> f64 {
    elevation.to_radians().tan()
}

/// How many pixels of the floor at the block's west foot (x = 14; the block is 4 high and spans z 14 to 18) are lit that its shadow covers, along the row `z`, seen from straight
/// above at `height` voxels over the floor with the sun at `elevation` degrees over the +x horizon, and how many pixels of the strip there were. The strip is from the foot to
/// `margin` short of where the shadow truly ends: a ray from a floor point at distance `d` from the foot reaches the wall's plane at `d tan(elevation)` over the floor, and
/// the wall is 4 high. A pixel is lit if it has at least half the light it has with the shadows off.
fn strip_holes(
    renderer: &mut VoxelRenderer,
    elevation: f64,
    height: i128,
    z: f64,
    margin: f64,
) -> (usize, usize) {
    let sun = sun_at(elevation);
    // Looking down as far as a camera can (a degree short of straight down): the camera is moved back so that the foot is where it looks, however high it is.
    let behind = height as f64 * elevation_tangent(90.0 - Camera::MAXIMUM_PITCH.to_degrees());
    let mut camera = Camera::new(place_in_voxels([14.0, (1 + height) as f64, z + behind]));
    camera.yaw = 0.0;
    camera.pitch = -Camera::MAXIMUM_PITCH;
    camera.vertical_field_of_view = (0.17 * 20.0 / height as f64) as f32;
    let (on, off) = shadows_on_and_off(renderer, sun, &camera);
    let true_end = 4.0 / elevation_tangent(elevation);
    let (mut holes, mut counted) = (0, 0);
    let mut last = None;
    for step in 1..=400 {
        let distance = f64::from(step) * 0.004;
        if distance > true_end - margin || distance > 1.5 {
            break;
        }
        let Some((x, y)) = pixel_of_sized(&camera, [-distance, -height as f64, -behind], BIG)
        else {
            continue;
        };
        if last == Some((x, y)) {
            continue;
        }
        last = Some((x, y));
        counted += 1;
        if big_brightness(&on, x, y) / big_brightness(&off, x, y) >= 0.5 {
            holes += 1;
        }
    }
    (holes, counted)
}

const ELEVATIONS: [f64; 9] = [5.0, 15.0, 30.0, 53.0, 60.0, 75.0, 80.0, 85.0, 88.0];
/// How high over the floor the camera is, which says which cascade of the map the floor is looked up in (the finest, the next, and a coarser one).
const HEIGHTS: [i128; 3] = [20, 60, 120];

#[test]
fn the_floor_is_dark_from_the_very_foot_of_a_wall_at_every_height_of_the_sun_and_in_every_cascade()
{
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = block_scene_renderer(&context);
        let mut without_the_volume = 0;
        for height in HEIGHTS {
            for elevation in ELEVATIONS {
                let (holes, counted) = strip_holes(&mut renderer, elevation, height, 16.0, 0.05);
                assert!(
                    counted >= 4,
                    "{name}: camera {height} up, sun {elevation}: only {counted} pixels in the strip"
                );
                assert_eq!(
                    holes, 0,
                    "{name}: camera {height} up, sun {elevation} degrees: {holes} of {counted} pixels at the foot of the wall are lit"
                );
            }
        }
        // The control: with the volume taken away the strip is there (so the test above can see it).
        renderer.clear_occupancy(ORIGIN_BRICK);
        for height in HEIGHTS {
            for elevation in ELEVATIONS {
                without_the_volume += strip_holes(&mut renderer, elevation, height, 16.0, 0.05).0;
            }
        }
        assert!(
            without_the_volume >= 60,
            "{name}: without the volume only {without_the_volume} pixels of the strip are lit, the test could not see a gap"
        );
    }
}

#[test]
fn the_floor_is_dark_at_the_foot_along_the_edge_rows_of_the_block_too() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = block_scene_renderer(&context);
        // (Rows at the middle of a cell, near the block's edge at z = 14 and its other edge at z = 18, and one in the middle: a wrong first step sideways leaves the block there.)
        let rows = [14.1, 14.5, 16.5, 17.5, 17.9];
        for z in rows {
            for elevation in [10.0, 30.0, 60.0, 80.0] {
                let (holes, counted) = strip_holes(&mut renderer, elevation, 20, z, 0.05);
                assert!(counted >= 4, "{name}: row {z}, sun {elevation}: {counted}");
                assert_eq!(
                    holes, 0,
                    "{name}: row z = {z}, sun {elevation} degrees: {holes} of {counted} pixels are lit"
                );
            }
        }
        // The control: without the volume the strip is there along these rows too.
        renderer.clear_occupancy(ORIGIN_BRICK);
        let without_the_volume: usize = rows
            .iter()
            .flat_map(|&z| [10.0, 30.0, 60.0, 80.0].map(move |elevation| (z, elevation)))
            .map(|(z, elevation)| strip_holes(&mut renderer, elevation, 20, z, 0.05).0)
            .sum();
        assert!(
            without_the_volume >= 30,
            "{name}: without the volume only {without_the_volume} pixels are lit, the test could not see a gap"
        );
    }
}

/// Solid rock with a closed room of x 8 to 23, y 4 to 9, z 8 to 23 carved out (its floor top is at y = 4, its east wall at x = 24).
fn room_cell(x: usize, y: usize, z: usize) -> Option<u32> {
    let inside = (8..24).contains(&x) && (4..10).contains(&y) && (8..24).contains(&z);
    (!inside).then_some(RED)
}

/// How many pixels of the floor of the room at the east wall's foot are lit with the volume `told` of the room, for the sun at `elevation`, and how many were looked at.
fn room_leaks(renderer: &mut VoxelRenderer, elevation: f64) -> (usize, usize) {
    let sun = sun_at(elevation);
    // Over the east wall's foot, looking straight down at the floor 4 voxels below.
    let height = 4i128;
    let mut camera = camera_in_cells(0, [24, 4 + height, 16], 0.0, -Camera::MAXIMUM_PITCH);
    camera.vertical_field_of_view = 0.85;
    let (on, off) = shadows_on_and_off(renderer, sun, &camera);
    let (mut lit, mut counted) = (0, 0);
    let mut last = None;
    for step in 1..=300 {
        let distance = f64::from(step) * 0.004;
        let Some((x, y)) = pixel_of_sized(&camera, [-distance, -height as f64, 0.0], BIG) else {
            continue;
        };
        if last == Some((x, y)) {
            continue;
        }
        last = Some((x, y));
        counted += 1;
        if big_brightness(&on, x, y) / big_brightness(&off, x, y) >= 0.5 {
            lit += 1;
        }
    }
    (lit, counted)
}

#[test]
fn no_sun_leaks_into_a_closed_room_of_rock_at_the_foot_of_its_wall() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let mut blocks = Vec::new();
        for z in 0..32usize {
            for y in 0..32usize {
                for x in 0..32usize {
                    if let Some(block) = room_cell(x, y, z) {
                        blocks.push(([x as u8, y as u8, z as u8], block, 0));
                    }
                }
            }
        }
        place_blocks(&mut renderer, ORIGIN_BRICK, &blocks, Some(0));
        let mut leaked_without_the_volume = 0;
        for elevation in [10.0, 30.0, 60.0, 85.0] {
            leaked_without_the_volume += room_leaks(&mut renderer, elevation).0;
        }
        assert!(
            leaked_without_the_volume >= 8,
            "{name}: the room leaks only {leaked_without_the_volume} pixels without the volume, the test could not see a leak"
        );
        renderer.set_occupancy(
            ORIGIN_BRICK,
            &occupancy_where(|x, y, z| room_cell(x, y, z).is_some()),
        );
        for elevation in [10.0, 30.0, 60.0, 85.0] {
            let (lit, counted) = room_leaks(&mut renderer, elevation);
            assert!(counted >= 20, "{name}: sun {elevation}: {counted} pixels");
            assert_eq!(
                lit, 0,
                "{name}: sun {elevation} degrees: {lit} of {counted} pixels of the floor at the wall's foot are lit in a closed room"
            );
        }
    }
}

#[test]
fn a_floor_nothing_stands_on_is_not_darkened_and_the_volume_never_makes_anything_brighter() {
    for context in contexts() {
        let name = adapter_name(&context);
        let floor: Vec<([u8; 3], u32, u32)> = (0..32u8)
            .flat_map(|z| (0..32u8).map(move |x| ([x, 0, z], 2, 0)))
            .collect();
        let mut renderer = new_renderer(&context);
        place_blocks(&mut renderer, ORIGIN_BRICK, &floor, Some(0));
        let mut camera = camera_at(ORIGIN_BRICK, [16.0, 12.0, 28.0]);
        camera.pitch = -0.6;
        for elevation in [5.0, 30.0, 60.0, 88.0] {
            let sun = sun_at(elevation);
            renderer.set_atmosphere(shadow_atmosphere(0, sun, true));
            let without = settled(&mut renderer, &camera);
            renderer.set_occupancy(ORIGIN_BRICK, &occupancy_where(|_, y, _| y == 0));
            let with = settled(&mut renderer, &camera);
            renderer.clear_occupancy(ORIGIN_BRICK);
            let lit = pixels_where(&with, |pixel| !is_sky(pixel));
            assert!(lit > 1000, "{name}: only {lit} pixels of floor");
            // (Two levels of a channel are the compiler's: the walk's variant of the shader is another program, which may round a sum its own way. A false shadow is a
            // great deal more.)
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (a, b) = (pixel(&with, x, y), pixel(&without, x, y));
                    assert!(
                        (0..3).all(|channel| a[channel].abs_diff(b[channel]) <= 2),
                        "{name}: sun {elevation}: pixel ({x}, {y}) of a floor with nothing on it is {a:?} with the volume and {b:?} without"
                    );
                }
            }
        }
        // And with a block on the floor the volume only ever takes light away.
        let mut scene = new_renderer(&context);
        upload_shadow_scene(&mut scene, ShadowScene::Block, 0);
        let mut camera = camera_at(ORIGIN_BRICK, [8.0, 12.0, 30.0]);
        camera.yaw = 0.35;
        camera.pitch = -0.5;
        for elevation in [10.0, 30.0, 60.0, 85.0] {
            scene.set_atmosphere(shadow_atmosphere(0, sun_at(elevation), true));
            let without = settled(&mut scene, &camera);
            scene.set_occupancy(
                ORIGIN_BRICK,
                &occupancy_where(|x, y, z| ShadowScene::Block.cell(x, y, z).is_some()),
            );
            let with = settled(&mut scene, &camera);
            scene.clear_occupancy(ORIGIN_BRICK);
            let mut darker = 0;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (a, b) = (pixel(&with, x, y), pixel(&without, x, y));
                    for channel in 0..3 {
                        assert!(
                            a[channel] <= b[channel].saturating_add(2),
                            "{name}: sun {elevation}: pixel ({x}, {y}) is brighter with the volume: {a:?} against {b:?}"
                        );
                    }
                    if (0..3).any(|channel| a[channel] + 20 < b[channel]) {
                        darker += 1;
                    }
                }
            }
            assert!(
                darker > 0,
                "{name}: sun {elevation}: the volume changed nothing in a scene with a wall"
            );
        }
    }
}

#[test]
fn the_walk_is_used_only_while_a_volume_holds_something_and_the_shadows_are_on() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        let mut camera = camera_at(ORIGIN_BRICK, [8.0, 12.0, 30.0]);
        camera.pitch = -0.5;
        let traced = |renderer: &mut VoxelRenderer| renderer.render_to_rgba(&camera, SIZE, SIZE).1;
        renderer.set_atmosphere(shadow_atmosphere(0, sun_at(30.0), true));
        assert_eq!(traced(&mut renderer).traced_windows, 0, "{name}: no volume");
        // Bricks that hold nothing that blocks the sun are nothing to trace.
        renderer.set_occupancy(ORIGIN_BRICK, &BrickOccupancy::Empty);
        assert_eq!(
            traced(&mut renderer).traced_windows,
            0,
            "{name}: empty bricks only"
        );
        renderer.set_occupancy(
            ORIGIN_BRICK,
            &occupancy_where(|x, y, z| ShadowScene::Block.cell(x, y, z).is_some()),
        );
        assert_eq!(traced(&mut renderer).traced_windows, 1, "{name}: a volume");
        // No shadows, no sun's shadow to correct.
        renderer.set_atmosphere(shadow_atmosphere(0, sun_at(30.0), false));
        assert_eq!(
            traced(&mut renderer).traced_windows,
            0,
            "{name}: shadows off"
        );
        renderer.set_atmosphere(shadow_atmosphere(0, sun_at(30.0), true));
        assert_eq!(traced(&mut renderer).traced_windows, 1, "{name}: back on");
        let statistics = renderer.occupancy_statistics();
        assert_eq!((statistics.windows, statistics.mixed), (1, 1), "{name}");
        renderer.clear_occupancy(ORIGIN_BRICK);
        assert_eq!(
            traced(&mut renderer).traced_windows,
            0,
            "{name}: the brick is gone"
        );
        assert_eq!(renderer.occupancy_statistics().windows, 0, "{name}");
        // A grid has a window of its own, and takes it with it when it goes.
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        renderer.set_grid_occupancy(a_ship(), ORIGIN_BRICK, &BrickOccupancy::Full);
        assert_eq!(renderer.occupancy_statistics().windows, 1, "{name}");
        renderer.remove_grid(a_ship());
        assert_eq!(renderer.occupancy_statistics().windows, 0, "{name}");
    }
}

// ---- the walk against the exact answer ----

/// A small deterministic source of random numbers (splitmix64): the tests must give the same probes every time, and on every adapter.
pub(super) struct Random(pub(super) u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    /// A number from 0 up to but not including 1.
    pub(super) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(super) fn below(&mut self, count: usize) -> usize {
        (self.next() % count as u64) as usize
    }
}

/// A brick of random voxels that block the sun, with a brick that is full to its +x, an empty one to its -x and none known anywhere else: all three kinds of entry of a table,
/// and a brick that is not known.
struct Soup {
    /// The brick the soup is in, in the grid it is mounted in.
    brick: [i128; 3],
    solid: Vec<bool>,
}

impl Soup {
    fn new(random: &mut Random, density: f64) -> Self {
        let mut solid = vec![false; 32 * 32 * 32];
        for z in 2..30usize {
            for y in 2..30usize {
                for x in 2..30usize {
                    solid[(z * 32 + y) * 32 + x] = random.unit() < density;
                }
            }
        }
        Self {
            brick: [0, 0, 0],
            solid,
        }
    }

    /// The same soup in another brick (and the full and empty bricks beside it with it).
    fn in_brick(mut self, brick: [i128; 3]) -> Self {
        self.brick = brick;
        self
    }

    fn brick_coordinate(&self, offset: [i128; 3]) -> BrickCoordinate {
        BrickCoordinate::new(
            self.brick[0] + offset[0],
            self.brick[1] + offset[1],
            self.brick[2] + offset[2],
        )
    }

    fn at(&self, x: usize, y: usize, z: usize) -> bool {
        self.solid[(z * 32 + y) * 32 + x]
    }

    /// Whether a voxel (in the brick's grid) blocks the sun, or `None` where nothing is known.
    fn blocks(&self, voxel: [i128; 3]) -> Option<bool> {
        let relative: [i128; 3] =
            std::array::from_fn(|axis| voxel[axis].div_euclid(32) - self.brick[axis]);
        let inside = |axis: usize| voxel[axis].rem_euclid(32) as usize;
        match relative {
            [0, 0, 0] => Some(self.at(inside(0), inside(1), inside(2))),
            [1, 0, 0] => Some(true),
            [-1, 0, 0] => Some(false),
            _ => None,
        }
    }

    fn occupancy(&self) -> BrickOccupancy {
        occupancy_where(|x, y, z| self.at(x, y, z))
    }

    fn voxels(&self) -> Vec<([u8; 3], u32)> {
        let mut voxels = Vec::new();
        for z in 0..32usize {
            for y in 0..32usize {
                for x in 0..32usize {
                    if self.at(x, y, z) {
                        voxels.push(([x as u8, y as u8, z as u8], RED));
                    }
                }
            }
        }
        voxels
    }
}

/// Where the soup stands: in the terrain, or in a grid that is turned.
#[derive(Clone, Copy)]
enum Mount {
    Terrain,
    Grid(Orientation),
}

impl Mount {
    /// The turn from the soup's axes to the world's, `[row][column]`.
    fn rotation(self) -> [[f64; 3]; 3] {
        match self {
            Mount::Terrain => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Mount::Grid(orientation) => {
                GridPose::new(WorldPosition::new(ORIGIN_BRICK, [0.0; 3]), orientation)
                    .rotation_matrix()
            }
        }
    }

    fn to_world(self, vector: [f64; 3]) -> [f64; 3] {
        let matrix = self.rotation();
        std::array::from_fn(|row| {
            (0..3)
                .map(|column| matrix[row][column] * vector[column])
                .sum()
        })
    }

    fn to_local(self, vector: [f64; 3]) -> [f64; 3] {
        let matrix = self.rotation();
        std::array::from_fn(|column| (0..3).map(|row| matrix[row][column] * vector[row]).sum())
    }

    /// A renderer with the soup in it, mounted this way.
    fn renderer_with(self, context: &GraphicsContext, soup: &Soup) -> VoxelRenderer {
        let mut renderer = new_renderer(context);
        if let Mount::Grid(orientation) = self {
            renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], orientation));
        }
        self.add(&mut renderer, soup);
        renderer
    }

    /// Puts a soup, with the full brick to its +x and the empty one to its -x, into a renderer.
    fn add(self, renderer: &mut VoxelRenderer, soup: &Soup) {
        let mesh = mesh_of(&soup.voxels());
        let (here, full, empty) = (
            soup.brick_coordinate([0, 0, 0]),
            soup.brick_coordinate([1, 0, 0]),
            soup.brick_coordinate([-1, 0, 0]),
        );
        match self {
            Mount::Terrain => {
                renderer.upload_brick(here, &mesh);
                renderer.set_occupancy(here, &soup.occupancy());
                renderer.set_occupancy(full, &BrickOccupancy::Full);
                renderer.set_occupancy(empty, &BrickOccupancy::Empty);
            }
            Mount::Grid(_) => {
                renderer.upload_grid_brick(a_ship(), here, &mesh);
                renderer.set_grid_occupancy(a_ship(), here, &soup.occupancy());
                renderer.set_grid_occupancy(a_ship(), full, &BrickOccupancy::Full);
                renderer.set_grid_occupancy(a_ship(), empty, &BrickOccupancy::Empty);
            }
        }
    }
}

/// A place on a face of the soup, and the camera that looks at it.
struct Probe {
    /// The cell in front of the face, in the soup's axes, and where the place is in it (0 to 1 along each axis; on the face, so 0 or 1 along the face's own).
    front: [i128; 3],
    fraction: [f64; 3],
    /// The two axes along the face.
    across: [usize; 2],
    eye: [f64; 3],
    target: [f64; 3],
}

/// A random probe: a face of a voxel of the soup that looks toward the sun (`toward_sun`, in the soup's axes), in the open (nothing within four cells in front of it), at a place
/// well inside it.
fn random_probe(random: &mut Random, soup: &Soup, toward_sun: [f64; 3]) -> Probe {
    loop {
        let voxel = [
            2 + random.below(28),
            2 + random.below(28),
            2 + random.below(28),
        ];
        if !soup.at(voxel[0], voxel[1], voxel[2]) {
            continue;
        }
        let axis = random.below(3);
        let sign: i128 = if random.below(2) == 0 { 1 } else { -1 };
        if toward_sun[axis] * sign as f64 <= 0.05 {
            continue;
        }
        // (In the grid's own voxels from here on.)
        let voxel: [i128; 3] =
            std::array::from_fn(|axis| voxel[axis] as i128 + soup.brick[axis] * 32);
        let step = |cells: i128| {
            let mut cell = voxel;
            cell[axis] += sign * cells;
            cell
        };
        if (1..=4).any(|cells| soup.blocks(step(cells)) == Some(true)) {
            continue;
        }
        let front = step(1);
        let across = [(axis + 1) % 3, (axis + 2) % 3];
        let mut fraction = [0.0; 3];
        fraction[axis] = if sign > 0 { 0.0 } else { 1.0 };
        for part in across {
            fraction[part] = 0.1 + 0.8 * random.unit();
        }
        let target: [f64; 3] = std::array::from_fn(|part| front[part] as f64 + fraction[part]);
        let mut eye = target;
        eye[axis] += 3.0 * sign as f64;
        return Probe {
            front,
            fraction,
            across,
            eye,
            target,
        };
    }
}

/// The exact answer for the probe, if it does not change when the place moves by up to a hundredth of a voxel across the face or the sun by a tenth of a thousandth (the shader
/// works in `f32` and a pixel is a part of a voxel across): whether the sun is blocked within `cells` cells.
fn exact_answer(soup: &Soup, probe: &Probe, toward_sun: [f64; 3], cells: u32) -> Option<bool> {
    let front = VoxelCoordinate::new(probe.front[0], probe.front[1], probe.front[2]);
    let walk = |fraction: [f64; 3], sun: [f64; 3]| {
        sun_blocked(front, fraction, sun, cells, |cell| {
            soup.blocks([cell.x(), cell.y(), cell.z()])
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
        // (A part that is exactly zero stays zero: that is a different ray, not a rounding error.)
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

/// The suns (toward the sun, in the world) the walk is tried with: some with a part of exactly zero (a ray along a lattice plane, or along an axis: the case a first
/// version of the shader got wrong), some through a diagonal, some neither.
const SUNS: [[f32; 3]; 12] = [
    [0.6, 0.8, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 2.0, -3.0],
    [0.3, 0.5, -0.8],
    [-0.7, 0.1, 0.2],
    [0.01, 0.2, 0.98],
    [1.0, 0.0, 0.0],
    [0.0, 0.0, -1.0],
];

/// What the walk of the shader finds for the probe: whether the sun is blocked, as `ContactDebug::Answer` draws it (white: not; black: blocked).
fn shader_answer(renderer: &mut VoxelRenderer, probe: &Probe, mount: Mount) -> bool {
    let eye = mount.to_world(probe.eye);
    let target = mount.to_world(probe.target);
    let camera = camera_aimed_at(eye, target);
    let image = settled_at(renderer, &camera, SIZE);
    let middle = pixel(&image, SIZE / 2, SIZE / 2);
    assert!(
        middle[..3] == [0, 0, 0] || middle[..3] == [255, 255, 255],
        "the answer is not black or white at the middle: {middle:?} (eye {eye:?}, target {target:?})"
    );
    middle[0] == 0
}

/// Lights the renderer's scene with the sun in this direction, for the walk's answer to be looked at.
fn set_sun_for_the_walk(renderer: &mut VoxelRenderer, sun: [f32; 3]) {
    renderer.set_atmosphere(Atmosphere {
        sky_color: SKY,
        fog_start: 1000.0,
        fog_end: 2000.0,
        sun_direction: sun,
        ..Atmosphere::default()
    });
}

/// What to try: the sun's direction (in the world), how many cells to walk, and how many random probes to take.
#[derive(Clone, Copy)]
struct Trial {
    sun: [f32; 3],
    cells: u32,
    probes: usize,
}

/// Tries the walk of `trial.cells` cells with `trial.probes` random probes at the soup (`renderer` holds it mounted as `mount`, and is already lit by the trial's sun): each probe's
/// answer is the reference's, or the test fails. Returns how many were blocked and how many were clear.
fn probes_against_the_reference(
    name: &str,
    renderer: &mut VoxelRenderer,
    mount: Mount,
    soup: &Soup,
    random: &mut Random,
    trial: Trial,
) -> (usize, usize) {
    let Trial { sun, cells, probes } = trial;
    // The sun as the renderer has it (normalised in `f32`), turned into the soup's axes.
    let world_sun = Vec3::from_array(sun).normalize();
    let toward_sun = mount.to_local(world_sun.to_array().map(f64::from));
    renderer.set_contact_debug(ContactDebug::Answer { cells });
    let (mut blocked, mut clear) = (0, 0);
    let mut attempts = 0;
    while blocked + clear < probes {
        attempts += 1;
        assert!(attempts < probes * 400, "{name}: too few probes are robust");
        let probe = random_probe(random, soup, toward_sun);
        let Some(exact) = exact_answer(soup, &probe, toward_sun, cells) else {
            continue;
        };
        let shader = shader_answer(renderer, &probe, mount);
        assert_eq!(
            shader,
            exact,
            "{name}: sun {sun:?}, {cells} cells, front cell {:?}, place {:?} in it: the shader says {}, the reference {}",
            probe.front,
            probe.fraction,
            if shader { "blocked" } else { "clear" },
            if exact { "blocked" } else { "clear" },
        );
        if exact {
            blocked += 1;
        } else {
            clear += 1;
        }
    }
    renderer.set_contact_debug(ContactDebug::Off);
    (blocked, clear)
}

/// Tries the walk with `probes` random probes for each sun and each number of cells, on a soup in this mount; returns how many were blocked and how many were clear.
fn walk_against_the_reference(
    context: &GraphicsContext,
    mount: Mount,
    cells: &[u32],
    suns: &[[f32; 3]],
    probes: usize,
    seed: u64,
) -> (usize, usize) {
    let name = adapter_name(context);
    let mut random = Random(seed);
    let soup = Soup::new(&mut random, 0.22);
    let mut renderer = mount.renderer_with(context, &soup);
    let (mut blocked, mut clear) = (0, 0);
    for &sun in suns {
        set_sun_for_the_walk(&mut renderer, sun);
        for &cell_count in cells {
            let (found_blocked, found_clear) = probes_against_the_reference(
                &name,
                &mut renderer,
                mount,
                &soup,
                &mut random,
                Trial {
                    sun,
                    cells: cell_count,
                    probes,
                },
            );
            blocked += found_blocked;
            clear += found_clear;
        }
    }
    (blocked, clear)
}

#[test]
fn the_shader_walk_finds_what_the_reference_finds_in_the_terrain() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (blocked, clear) =
            walk_against_the_reference(&context, Mount::Terrain, &[1, 3, 8], &SUNS, 14, 0x5EED);
        // (So that the test is not about one answer: both happen a good deal.)
        assert!(
            blocked >= 100 && clear >= 100,
            "{name}: {blocked} blocked, {clear} clear"
        );
    }
}

#[test]
fn the_shader_walk_finds_what_the_reference_finds_in_a_grid_that_is_turned() {
    for context in contexts() {
        let name = adapter_name(&context);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        for (turn, orientation) in [
            ("half turn about y", half_turn_about_y()),
            ("half turn about x", half_turn_about_x()),
            (
                "quarter turn about y",
                Orientation::from_components([0.0, half, 0.0, half]).unwrap(),
            ),
            (
                "a turn about a slanted axis",
                Orientation::from_components([0.3, 0.4, 0.0, (1.0f64 - 0.09 - 0.16).sqrt()])
                    .unwrap(),
            ),
        ] {
            let (blocked, clear) = walk_against_the_reference(
                &context,
                Mount::Grid(orientation),
                &[2, 6],
                &SUNS[..8],
                6,
                0xC0FFEE,
            );
            assert!(
                blocked >= 10 && clear >= 10,
                "{name}, {turn}: {blocked} blocked, {clear} clear"
            );
        }
    }
}

#[test]
fn two_grids_in_view_at_once_are_each_traced_in_a_window_of_their_own() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut random = Random(0x2B);
        // The terrain has a soup in the brick (3, 0, 0), and a ship, not turned, one in its own first brick: the world voxels 0 to 32 in each axis.
        let terrain = Soup::new(&mut random, 0.22).in_brick([3, 0, 0]);
        let ship = Soup::new(&mut random, 0.22);
        let turn = Mount::Grid(Orientation::IDENTITY);
        let mut renderer = new_renderer(&context);
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        turn.add(&mut renderer, &ship);
        Mount::Terrain.add(&mut renderer, &terrain);
        assert_eq!(renderer.occupancy_statistics().windows, 2, "{name}");
        for sun in [[0.6, 0.8, 0.0], [-1.0, 2.0, -3.0], [1.0, 0.0, 1.0]] {
            set_sun_for_the_walk(&mut renderer, sun);
            for (mount, soup) in [(Mount::Terrain, &terrain), (turn, &ship)] {
                let trial = Trial {
                    sun,
                    cells: 5,
                    probes: 12,
                };
                let (blocked, clear) = probes_against_the_reference(
                    &name,
                    &mut renderer,
                    mount,
                    soup,
                    &mut random,
                    trial,
                );
                assert!(blocked > 0 && clear > 0, "{name}: {blocked} and {clear}");
            }
        }
    }
}

#[test]
fn the_pool_grows_when_it_is_full_and_what_it_held_is_kept() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut random = Random(0xB16);
        let first = Soup::new(&mut random, 0.22);
        let second = Soup::new(&mut random, 0.22).in_brick([0, 0, 2]);
        let mut renderer = Mount::Terrain.renderer_with(&context, &first);
        let suns = [[0.6, 0.8, 0.0], [-1.0, 2.0, -3.0]];
        let try_both = |renderer: &mut VoxelRenderer, random: &mut Random, soups: &[&Soup]| {
            for soup in soups {
                for sun in suns {
                    set_sun_for_the_walk(renderer, sun);
                    let trial = Trial {
                        sun,
                        cells: 6,
                        probes: 12,
                    };
                    let (blocked, clear) = probes_against_the_reference(
                        &name,
                        renderer,
                        Mount::Terrain,
                        soup,
                        random,
                        trial,
                    );
                    assert!(blocked > 0 && clear > 0, "{name}: {blocked} and {clear}");
                }
            }
        };
        // The soup in the first slot of a pool that has room for 256, and it is right.
        try_both(&mut renderer, &mut random, &[&first]);
        // More mixed bricks than that, so the pool has to grow (and copy what it holds), then a soup in a slot the first pool had no room for.
        for index in 0..400i128 {
            let coordinate = BrickCoordinate::new(-8 + index % 16, 6 + index / 16, -3);
            let place = index as usize % 32;
            renderer.set_occupancy(
                coordinate,
                &occupancy_where(|x, y, z| (x, y, z) == (place, 3, 4)),
            );
        }
        Mount::Terrain.add(&mut renderer, &second);
        let statistics = renderer.occupancy_statistics();
        assert!(
            statistics.mixed >= 402 && statistics.slots_reached >= 402,
            "{name}: {statistics:?}"
        );
        try_both(&mut renderer, &mut random, &[&first, &second]);
    }
}

#[test]
fn a_brick_far_from_the_camera_is_never_read_as_the_one_a_window_nearer_that_shares_its_place() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        // A floor in the top voxels of the brick (40, 0, 0), 1,280 voxels along x from the brick the camera is in: its top is at y = 32 and the cell in front of it is in the
        // brick (40, 1, 0), which the camera's window does not reach. That brick shares its place in the table with (-24, 1, 0), which is told to be full.
        let floor: Vec<([u8; 3], u32)> = (0..32u8)
            .flat_map(|z| (0..32u8).map(move |x| ([x, 31, z], RED)))
            .collect();
        renderer.upload_brick(BrickCoordinate::new(40, 0, 0), &mesh_of(&floor));
        renderer.set_occupancy(BrickCoordinate::new(-24, 1, 0), &BrickOccupancy::Full);
        set_sun_for_the_walk(&mut renderer, [0.0, 1.0, 0.0]);
        renderer.set_contact_debug(ContactDebug::Answer { cells: 4 });
        let middle_of = |renderer: &mut VoxelRenderer, eye: [f64; 3]| {
            let camera = camera_aimed_at(eye, [40.0 * 32.0 + 16.0, 32.0, 16.0]);
            pixel(&settled_at(renderer, &camera, SIZE), SIZE / 2, SIZE / 2)
        };
        let far = middle_of(&mut renderer, [0.0, 600.0, 0.0]);
        assert_eq!(
            far[..3],
            [255, 255, 255],
            "{name}: seen from 40 bricks away the floor is in the shadow of a brick it is not near"
        );
        // The control: told of its own brick, and seen from beside it, the floor is in shadow (the walk enters the full brick at once).
        renderer.set_occupancy(BrickCoordinate::new(40, 1, 0), &BrickOccupancy::Full);
        let near = middle_of(&mut renderer, [40.0 * 32.0 + 16.0, 46.0, 16.0 + 4.0]);
        assert_eq!(
            near[..3],
            [0, 0, 0],
            "{name}: seen from beside it the floor under a full brick is lit"
        );
    }
}

/// The scene of the shadow tests in a ship turned half way round about the vertical axis. The scene is the same world under that turn (the floor and the block are the same
/// cells seen from behind), so the pictures of the strip are asked the same questions; the face at the block's west foot is, in the ship, the one at its east foot.
fn block_scene_in_a_ship_renderer(context: &GraphicsContext) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    renderer.set_grid_pose(a_ship(), pose([1, 0, 1], [0.0; 3], half_turn_about_y()));
    let mut blocks = Vec::new();
    for z in 0..32usize {
        for y in 0..32usize {
            for x in 0..32usize {
                if let Some(block) = ShadowScene::Block.cell(x, y, z) {
                    blocks.push(([x as u8, y as u8, z as u8], block, 0));
                }
            }
        }
    }
    renderer.upload_grid_brick(
        a_ship(),
        ORIGIN_BRICK,
        &mesh_of_blocks(&shaped_table(), &blocks, Some(0)),
    );
    renderer.set_grid_occupancy(
        a_ship(),
        ORIGIN_BRICK,
        &occupancy_where(|x, y, z| ShadowScene::Block.cell(x, y, z).is_some()),
    );
    renderer
}

#[test]
fn the_floor_is_dark_from_the_foot_of_a_wall_of_a_ship_turned_half_way_round_too() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = block_scene_in_a_ship_renderer(&context);
        let suns = [5.0, 30.0, 60.0, 85.0];
        for height in [20, 60] {
            for elevation in suns {
                let (holes, counted) = strip_holes(&mut renderer, elevation, height, 16.0, 0.05);
                assert!(
                    counted >= 4,
                    "{name}: camera {height} up, sun {elevation}: {counted}"
                );
                assert_eq!(
                    holes, 0,
                    "{name}: ship, camera {height} up, sun {elevation} degrees: {holes} of {counted} pixels at the foot are lit"
                );
            }
        }
        renderer.clear_grid_occupancy(a_ship(), ORIGIN_BRICK);
        let without_the_volume: usize = [20, 60]
            .into_iter()
            .flat_map(|height| suns.map(move |elevation| (height, elevation)))
            .map(|(height, elevation)| strip_holes(&mut renderer, elevation, height, 16.0, 0.05).0)
            .sum();
        assert!(
            without_the_volume >= 20,
            "{name}: without the volume only {without_the_volume} pixels are lit, the test could not see a gap"
        );
    }
}

#[test]
fn the_walk_does_not_start_from_a_face_that_looks_away_from_the_sun() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        // Two cubes: the second is up and to the east of the first, in the way of a ray from the first's top.
        let cubes = [([16, 16, 16], 1, 0), ([17, 17, 16], 1, 0)];
        place_blocks(&mut renderer, ORIGIN_BRICK, &cubes, Some(0));
        renderer.set_occupancy(
            ORIGIN_BRICK,
            &occupancy_where(|x, y, z| (x, y, z) == (16, 16, 16) || (x, y, z) == (17, 17, 16)),
        );
        // The sun is up and to the east.
        set_sun_for_the_walk(&mut renderer, [0.6, 0.8, 0.0]);
        renderer.set_contact_debug(ContactDebug::Answer { cells: 4 });
        let answer = |renderer: &mut VoxelRenderer, eye: [f64; 3], target: [f64; 3]| {
            let camera = camera_aimed_at(eye, target);
            pixel(&settled_at(renderer, &camera, SIZE), SIZE / 2, SIZE / 2)
        };
        // The top of the first cube looks at the sun and the second cube is in the way of its ray: black. (The control: this is a face the walk is made for.)
        let top = answer(&mut renderer, [16.5, 22.0, 16.7], [16.5, 17.0, 16.5]);
        assert_eq!(top[..3], [0, 0, 0], "{name}: the top of the cube");
        // Its west face looks away from the sun, and a ray from it would enter the cube at once: it is not walked, which is white.
        let west = answer(&mut renderer, [12.0, 16.5, 16.5], [16.0, 16.5, 16.5]);
        assert_eq!(
            west[..3],
            [255, 255, 255],
            "{name}: the west face of the cube"
        );
        // Nor is its underside.
        let underneath = answer(&mut renderer, [16.5, 11.0, 16.7], [16.5, 16.0, 16.5]);
        assert_eq!(
            underneath[..3],
            [255, 255, 255],
            "{name}: the underside of the cube"
        );
    }
}

// ---- the cost ----

/// Whether the voxel at `(x, y, z)` of the bench's 96 by 32 by 96 box is a pillar: about one column in eight stands, one to five voxels high; y = 0 is the floor.
fn bench_solid(x: u32, y: u32, z: u32) -> Option<u32> {
    if y == 0 {
        return Some(2);
    }
    let mut hash = x.wrapping_mul(0x9E37_79B1) ^ z.wrapping_mul(0x85EB_CA6B);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2C1B_3C6D);
    hash ^= hash >> 12;
    ((hash & 7) == 0 && y <= 1 + (hash >> 8) % 5).then_some(RED)
}

#[test]
#[ignore = "a measurement, not a test: cargo test --release -p renderer --test gpu_rendering cost_of_the_walk -- --ignored --nocapture"]
fn cost_of_the_walk_on_a_scene_of_many_contact_edges() {
    use std::time::Instant;
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let mut occupancies = Vec::new();
        for bz in 0..3i128 {
            for bx in 0..3i128 {
                let mut blocks = Vec::new();
                for z in 0..32u8 {
                    for y in 0..32u8 {
                        for x in 0..32u8 {
                            let (gx, gz) =
                                (bx as u32 * 32 + u32::from(x), bz as u32 * 32 + u32::from(z));
                            if let Some(block) = bench_solid(gx, u32::from(y), gz) {
                                blocks.push(([x, y, z], block, 0));
                            }
                        }
                    }
                }
                let brick = BrickCoordinate::new(bx, 0, bz);
                place_blocks(&mut renderer, brick, &blocks, Some(0));
                occupancies.push((
                    brick,
                    occupancy_where(|x, y, z| {
                        bench_solid(
                            bx as u32 * 32 + x as u32,
                            y as u32,
                            bz as u32 * 32 + z as u32,
                        )
                        .is_some()
                    }),
                ));
            }
        }
        for told in [false, true] {
            if told {
                for (brick, occupancy) in &occupancies {
                    renderer.set_occupancy(*brick, occupancy);
                }
            }
            let mut camera = camera_at(BrickCoordinate::new(1, 0, 2), [16.0, 12.0, 28.0]);
            camera.pitch = -0.45;
            let mut sun = glam::Vec3::new(0.6, 0.8, 0.0).normalize();
            let mut times = Vec::new();
            let mut traced = 0;
            for batch in 0..33 {
                let started = Instant::now();
                for _ in 0..10 {
                    camera.move_in_world(glam::DVec3::new(0.083, 0.0, -0.017));
                    sun = glam::Quat::from_rotation_z(-8.7e-5) * sun;
                    renderer.set_atmosphere(shadow_atmosphere(0, sun.to_array(), true));
                    traced = renderer.render_offscreen(&camera, 1280, 720).traced_windows;
                }
                renderer.wait_for_gpu();
                if batch >= 3 {
                    times.push(started.elapsed().as_secs_f64() * 100.0);
                }
            }
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            println!(
                "COST {name} | the walk {} (windows traced {traced}) | mean {mean:.2} ms, median {:.2}, p95 {:.2}",
                if told { "on" } else { "off" },
                times[times.len() / 2],
                times[(times.len() - 1) * 95 / 100]
            );
        }
    }
}
