//! The sun's exact short ray through the voxels of another grid (`docs/specs/contact-trace-spec.md`, section 14): the ground meets the shade of a ship that stands on it, and a
//! ship meets that of the ground.
//!
//! A face is walked in the window of its own grid, and, while two or more windows are traced, in the others': the ray is turned into the other grid's axes, starts in the cell the
//! place is in (a sixteenth of a voxel out of the face) and tests it, and a brick the other grid has nothing of is vacuum to it. These tests are of the shader's walk
//! (`ContactDebug`) against `world_view::sun_blocked` for the face's own grid together with `world_view::sun_blocked_from_point` for the other, on soups of voxels in two grids that
//! are turned against each other, that overlap and that are beside each other (the bricks between them are not known to the other's window); and of the picture: the floor is
//! dark from the very foot of a wall that stands on it and belongs to a ship (and is not without the ship's window, the control).

use world_view::{SunCell, sun_blocked, sun_blocked_from_point};

use super::contact_shapes::{ShapedSoup, shaped_blocks};
use super::contact_trace::{
    Mount, Probe, Random, SUNS, Soup, random_probe, set_sun_for_the_walk, shader_answer,
};
use super::*;

/// What a soup says of a voxel in the brick it is in and nowhere else (the bricks round it are not known): for a grid that is only an occluder, which has no neighbours of
/// the kind the probes' soups have.
fn blocks_inside(soup: &Soup, voxel: [i128; 3]) -> Option<SunCell> {
    let inside = |axis: usize| {
        (voxel[axis].div_euclid(32) == soup.brick[axis])
            .then(|| voxel[axis].rem_euclid(32) as usize)
    };
    let (x, y, z) = (inside(0)?, inside(1)?, inside(2)?);
    Some(if soup.at(x, y, z) {
        SunCell::Blocks
    } else {
        SunCell::Clear
    })
}

/// The place of the probe in the world, and the way its face looks (a unit vector), given how its soup is mounted.
fn probe_in_the_world(mount: Mount, probe: &Probe) -> ([f64; 3], [f64; 3]) {
    let axis = (0..3)
        .find(|axis| !probe.across.contains(axis))
        .expect("a face has an axis");
    let place: [f64; 3] =
        std::array::from_fn(|part| probe.front[part] as f64 + probe.fraction[part]);
    let mut normal = [0.0; 3];
    // (`random_probe` puts the place at 0 along the axis for a face that looks up it, and at 1 for one that looks down it.)
    normal[axis] = if probe.fraction[axis] == 0.0 {
        1.0
    } else {
        -1.0
    };
    (mount.to_world(place), mount.to_world(normal))
}

/// What the other grid says of a voxel in its own axes: `None` where it knows nothing.
type OtherCells<'a> = &'a dyn Fn([i128; 3]) -> Option<SunCell>;

/// Whether the sun is blocked at the probe by the face's own grid (`own`) or by the other (`other_mount`, `other_cells`), for the sun in the world and a place `fraction` in the
/// front cell.
fn blocked_at(
    own: (&Soup, Mount),
    (other_mount, other_cells): (Mount, OtherCells<'_>),
    probe: &Probe,
    fraction: [f64; 3],
    sun_world: [f64; 3],
    cells: u32,
) -> (bool, bool) {
    let front = VoxelCoordinate::new(probe.front[0], probe.front[1], probe.front[2]);
    let by_own = sun_blocked(front, fraction, own.1.to_local(sun_world), cells, |cell| {
        own.0.blocks([cell.x(), cell.y(), cell.z()])
    });
    let moved = Probe {
        front: probe.front,
        fraction,
        across: probe.across,
        eye: probe.eye,
        target: probe.target,
    };
    let (place, normal) = probe_in_the_world(own.1, &moved);
    // A sixteenth of a voxel out of the face, in the other grid's axes.
    let nudged: [f64; 3] = std::array::from_fn(|part| place[part] + normal[part] * 0.0625);
    let by_other = sun_blocked_from_point(
        other_mount.to_local(nudged),
        other_mount.to_local(sun_world),
        cells,
        |cell| other_cells([cell.x(), cell.y(), cell.z()]),
    );
    (by_own, by_other)
}

/// The reference's answer (blocked by either grid) and whether the other grid decided it, if the answer does not change when the place moves by up to a hundredth of a voxel
/// across the face or the sun by a tenth of a thousandth.
fn exact_answer_with_the_other(
    own: (&Soup, Mount),
    other: (Mount, OtherCells<'_>),
    probe: &Probe,
    sun_world: [f64; 3],
    cells: u32,
) -> Option<(bool, bool)> {
    let combined = |fraction: [f64; 3], sun: [f64; 3]| {
        let (by_own, by_other) = blocked_at(own, other, probe, fraction, sun, cells);
        (by_own || by_other, !by_own && by_other)
    };
    let answer = combined(probe.fraction, sun_world);
    for first in [-0.01, 0.0, 0.01] {
        for second in [-0.01, 0.0, 0.01] {
            let mut fraction = probe.fraction;
            fraction[probe.across[0]] += first;
            fraction[probe.across[1]] += second;
            if combined(fraction, sun_world).0 != answer.0 {
                return None;
            }
        }
    }
    for part in 0..3 {
        if sun_world[part] == 0.0 {
            continue;
        }
        for scale in [1.0 - 1e-4, 1.0 + 1e-4] {
            let mut sun = sun_world;
            sun[part] *= scale;
            if combined(probe.fraction, sun).0 != answer.0 {
                return None;
            }
        }
    }
    Some(answer)
}

/// What the other grid is: a soup of cubes, or (a ship only) a soup of cubes and shapes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Occluder {
    Cubes,
    Shapes,
}

/// What one trial asks: how the ship is turned, whose faces are probed, what the other grid is made of, and where the ground's brick is, as an offset from the world brick the
/// middle of the ship's brick is in (0, 0, 0 is on top of it, the others are beside it).
#[derive(Clone, Copy)]
struct Scene {
    orientation: Orientation,
    own_is_the_ship: bool,
    occluder: Occluder,
    ground_offset: [i128; 3],
}

/// Tries random probes at the faces of a soup of the face's own grid (meshed and mounted as the scene says), with a soup of the other grid mounted that is only an occluder (no
/// mesh: nobody sees it, and it cannot stand in the camera's way), for the suns and the cell counts given: each probe's answer is the reference's, or the test fails. Returns how
/// many were blocked, how many were clear, and how many the other grid decided (blocked by it, and not by the face's own).
fn grids_against_the_reference(
    context: &GraphicsContext,
    scene: Scene,
    cells: &[u32],
    suns: &[[f32; 3]],
    probes: usize,
    seed: u64,
) -> (usize, usize, usize) {
    let name = adapter_name(context);
    let mut random = Random(seed);
    let (ship_mount, ground_mount) = (Mount::Grid(scene.orientation), Mount::Terrain);
    let (own_mount, other_mount) = if scene.own_is_the_ship {
        (ship_mount, ground_mount)
    } else {
        (ground_mount, ship_mount)
    };
    // The ship's soup is in its brick 0, and the ground's in the world brick that the middle of that one is in, moved by the offset.
    let middle_brick: [i128; 3] = ship_mount
        .to_world([16.0; 3])
        .map(|part| (part / 32.0).floor() as i128);
    let ground_brick: [i128; 3] =
        std::array::from_fn(|axis| middle_brick[axis] + scene.ground_offset[axis]);
    let (own_brick, other_brick) = if scene.own_is_the_ship {
        ([0, 0, 0], ground_brick)
    } else {
        (ground_brick, [0, 0, 0])
    };
    let own = Soup::new(&mut random, 0.22).in_brick(own_brick);
    let mut renderer = new_renderer(context);
    renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], scene.orientation));
    own_mount.add(&mut renderer, &own);
    // The other grid, told of its one brick and no other.
    let table = shaped_blocks();
    let other_cells: Box<dyn Fn([i128; 3]) -> Option<SunCell>> = match scene.occluder {
        Occluder::Cubes => {
            let other = Soup::new(&mut random, 0.22).in_brick(other_brick);
            match other_mount {
                Mount::Terrain => {
                    renderer.set_occupancy(other.brick_coordinate([0, 0, 0]), &other.occupancy());
                }
                Mount::Grid(_) => renderer.set_grid_occupancy(
                    a_ship(),
                    other.brick_coordinate([0, 0, 0]),
                    &other.occupancy(),
                ),
            }
            Box::new(move |voxel| blocks_inside(&other, voxel))
        }
        Occluder::Shapes => {
            assert!(
                !scene.own_is_the_ship,
                "the ship is the one that has shapes"
            );
            let ship = ShapedSoup::new(&mut random, 0.06, 0.2);
            renderer.set_sun_shapes(&table.shape_cells());
            renderer.set_grid_occupancy(a_ship(), ORIGIN_BRICK, &ship.occupancy(&table));
            Box::new(move |voxel| ship.cell(&table, voxel))
        }
    };
    assert_eq!(renderer.occupancy_statistics().windows, 2, "{name}");
    let (mut blocked, mut clear, mut by_other) = (0, 0, 0);
    for &sun in suns {
        set_sun_for_the_walk(&mut renderer, sun);
        let sun_world = Vec3::from_array(sun).normalize().to_array().map(f64::from);
        let toward_sun = own_mount.to_local(sun_world);
        for &cell_count in cells {
            renderer.set_contact_debug(ContactDebug::Answer { cells: cell_count });
            let (mut found, mut attempts) = (0, 0);
            while found < probes {
                attempts += 1;
                assert!(attempts < probes * 800, "{name}: too few probes are robust");
                let probe = random_probe(&mut random, &own, toward_sun);
                let Some((exact, decided_by_the_other)) = exact_answer_with_the_other(
                    (&own, own_mount),
                    (other_mount, &*other_cells),
                    &probe,
                    sun_world,
                    cell_count,
                ) else {
                    continue;
                };
                let shader = shader_answer(&mut renderer, &probe, own_mount);
                assert_eq!(
                    shader,
                    exact,
                    "{name}: sun {sun:?}, {cell_count} cells, ground offset {:?}, front cell {:?}, place {:?}: the shader says {}, the reference {}{}",
                    scene.ground_offset,
                    probe.front,
                    probe.fraction,
                    if shader { "blocked" } else { "clear" },
                    if exact { "blocked" } else { "clear" },
                    if decided_by_the_other {
                        " (by the other grid)"
                    } else {
                        ""
                    },
                );
                found += 1;
                if exact {
                    blocked += 1;
                } else {
                    clear += 1;
                }
                if decided_by_the_other {
                    by_other += 1;
                }
            }
        }
    }
    renderer.set_contact_debug(ContactDebug::Off);
    (blocked, clear, by_other)
}

/// The sun's directions tried: those of the walk's own tests that have something to do with a ground and a ship that are turned against each other.
fn suns() -> Vec<[f32; 3]> {
    SUNS[..9].to_vec()
}

/// Where the ground's brick is, from the one on top of the ship's: on top of it, and beside it on each side and over and under it (where the bricks between the two grids' soups
/// are not known to the other's window, and the walk must cross them).
const GROUND_OFFSETS: [[i128; 3]; 7] = [
    [0, 0, 0],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 0, 1],
    [0, 0, -1],
    [0, 1, 0],
    [0, -1, 0],
];

/// The turns of the ship the tests try: none, a half and a quarter about the vertical axis, and one about a slanted axis (whose lattice is not that of the ground).
fn turns() -> Vec<(&'static str, Orientation)> {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    vec![
        ("not turned", Orientation::IDENTITY),
        ("half turn about y", half_turn_about_y()),
        (
            "a quarter turn about y",
            Orientation::from_components([0.0, half, 0.0, half]).unwrap(),
        ),
        (
            "a turn about a slanted axis",
            Orientation::from_components([0.3, 0.4, 0.0, (1.0f64 - 0.09 - 0.16).sqrt()]).unwrap(),
        ),
    ]
}

fn across_the_offsets(
    context: &GraphicsContext,
    turn: &str,
    orientation: Orientation,
    own_is_the_ship: bool,
    occluder: Occluder,
    seed: u64,
) {
    let name = adapter_name(context);
    let (mut blocked, mut clear, mut by_other) = (0, 0, 0);
    for ground_offset in GROUND_OFFSETS {
        let (found_blocked, found_clear, found_by_other) = grids_against_the_reference(
            context,
            Scene {
                orientation,
                own_is_the_ship,
                occluder,
                ground_offset,
            },
            &[2, 6],
            &suns(),
            8,
            seed,
        );
        blocked += found_blocked;
        clear += found_clear;
        by_other += found_by_other;
    }
    assert!(
        blocked >= 100 && clear >= 100 && by_other >= 12,
        "{name}, {turn}: {blocked} blocked, {clear} clear, {by_other} by the other grid: the test cannot see the other grid"
    );
}

#[test]
fn the_ground_meets_the_shade_of_a_ship_and_the_shader_finds_what_the_reference_finds() {
    for context in contexts() {
        for (turn, orientation) in turns() {
            across_the_offsets(&context, turn, orientation, false, Occluder::Cubes, 0x6A0D);
        }
    }
}

#[test]
fn a_ship_meets_the_shade_of_the_ground_and_the_shader_finds_what_the_reference_finds() {
    for context in contexts() {
        for (turn, orientation) in turns() {
            across_the_offsets(&context, turn, orientation, true, Occluder::Cubes, 0x5417);
        }
    }
}

#[test]
fn the_ground_meets_the_shade_of_a_ship_of_slabs_stairs_and_posts_too() {
    for context in contexts() {
        for (turn, orientation) in turns() {
            across_the_offsets(&context, turn, orientation, false, Occluder::Shapes, 0x51AF);
        }
    }
}

/// The floor of cubes of the terrain, one voxel thick (its top at y = 1), and a ship, not turned, whose brick holds a wall of four by four voxels and four high at x 14 to 17 and z
/// 14 to 17 standing on it: the scene of the first test of the walk, in two grids.
fn ship_on_a_floor(context: &GraphicsContext) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
    // (The terrain keeps its floor and loses the block: the block is the ship's.)
    let floor = super::contact_trace::occupancy_where(|_, y, _| y == 0);
    renderer.set_occupancy(ORIGIN_BRICK, &floor);
    let wall: Vec<([u8; 3], u32)> = (14..18u8)
        .flat_map(|z| (1..5u8).flat_map(move |y| (14..18u8).map(move |x| ([x, y, z], GREEN))))
        .collect();
    renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
    renderer.upload_grid_brick(a_ship(), ORIGIN_BRICK, &mesh_of(&wall));
    renderer.set_grid_occupancy(
        a_ship(),
        ORIGIN_BRICK,
        &super::contact_trace::occupancy_where(|x, y, z| {
            (14..18).contains(&x) && (1..5).contains(&y) && (14..18).contains(&z)
        }),
    );
    renderer
}

#[test]
fn the_floor_is_dark_from_the_very_foot_of_a_wall_that_belongs_to_a_ship_standing_on_it_and_not_without_the_ships_window()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = ship_on_a_floor(&context);
        let mut walked_others = false;
        for height in [20, 60] {
            for elevation in [5.0, 15.0, 30.0, 53.0, 75.0] {
                let (holes, counted) =
                    super::contact_trace::strip_holes(&mut renderer, elevation, height, 16.0, 0.05);
                assert!(
                    counted >= 4,
                    "{name}: camera {height} up, sun {elevation}: {counted}"
                );
                assert_eq!(
                    holes, 0,
                    "{name}: camera {height} up, sun {elevation} degrees: {holes} of {counted} pixels at the foot of the ship's wall are lit"
                );
            }
        }
        let statistics = renderer
            .render_to_rgba(&camera_at(ORIGIN_BRICK, [8.0, 12.0, 30.0]), SIZE, SIZE)
            .1;
        walked_others |= statistics.walked_other_windows;
        // The control: without the ship's window the ground does not know the wall is there, and the strip is.
        renderer.clear_grid_occupancy(a_ship(), ORIGIN_BRICK);
        let mut lit_without = 0;
        for height in [20, 60] {
            for elevation in [5.0, 15.0, 30.0, 53.0, 75.0] {
                lit_without +=
                    super::contact_trace::strip_holes(&mut renderer, elevation, height, 16.0, 0.05)
                        .0;
            }
        }
        assert!(
            lit_without >= 40,
            "{name}: without the ship's window only {lit_without} pixels of the strip are lit, the test could not see a gap"
        );
        // With one window left the walk is the one that has nothing to do with the others, and the shader is the one without that code.
        let statistics = renderer
            .render_to_rgba(&camera_at(ORIGIN_BRICK, [8.0, 12.0, 30.0]), SIZE, SIZE)
            .1;
        assert_eq!(
            (statistics.traced_windows, statistics.walked_other_windows),
            (1, false),
            "{name}: one window"
        );
        assert!(
            walked_others,
            "{name}: the walk through other grids was not the one drawn"
        );
    }
}
